#!/usr/bin/env python3
"""Bench utility: set mechanical zero + save params for a single RobStride RS03.

Scope: commissioning / one-motor-on-the-bench only. This is NOT the driver.
The real driver (src/driver) will do its own framing with proper bus
arbitration, timeouts, and ACK handling. This script exists so that Step 5 and
Step 7 of tools/robstride/commission.md can be executed from the Pi without
Motor Studio, which is useful because:

  - Motor Studio v0.0.13 (the one on vendor USB sticks) has no "save
    parameters" button.
  - Even Motor Studio v1.0.3 is a Windows-only GUI; for a 2-motor
    shoulder commissioning session it's faster to script from the Pi.

Usage (on Pi, one motor on can0 at the specified CAN ID):

    sudo ./tools/robstride/bench_set_zero_and_save.py --iface can0 \\
        --motor-id 0x08 --host-id 0xFD --read-only

    sudo ./tools/robstride/bench_set_zero_and_save.py --iface can0 \\
        --motor-id 0x08 --host-id 0xFD --set-zero --save

Safety:
  - Refuses to run if the rotor is spinning (speed > 0.05 rad/s).
  - Refuses to run if faultSta != 0.
  - Always reads-back MechOffset before and after.
  - Does NOT enable the motor; does NOT send MIT frames.

Refs: docs/decisions/0002-rs03-protocol-spec.md
      docs/vendor/rs03-user-manual-260112.pdf §4.1
"""

from __future__ import annotations

import argparse
import socket
import struct
import sys
import time

# --- constants from ADR-0002 -------------------------------------------------

COMM_TYPE_READ_PARAM = 0x11
COMM_TYPE_WRITE_PARAM = 0x12
COMM_TYPE_SET_ZERO = 0x06
COMM_TYPE_SAVE_PARAMS = 0x16
COMM_TYPE_STOP = 0x04

PARAM_MECH_OFFSET = 0x2005
PARAM_FAULT_STA = 0x3022
PARAM_MECH_POS = 0x3016
PARAM_MECH_VEL = 0x3017
PARAM_CAN_ID = 0x2009
PARAM_APP_CODE_VERSION = 0x1003

CAN_EFF_FLAG = 0x80000000  # Linux SocketCAN extended-frame flag
SOCKET_TIMEOUT_S = 0.5
SAVE_SETTLE_S = 0.2
POWER_SANITY_MAX_VEL_RAD_S = 0.05


# --- CAN framing -------------------------------------------------------------

def arb_id(comm_type: int, host_id: int, motor_id: int) -> int:
    """Build the 29-bit arbitration ID per ADR-0002."""
    if not 0 <= comm_type <= 0x1F:
        raise ValueError(f"comm_type {comm_type} out of 5-bit range")
    if not 0 <= host_id <= 0xFF:
        raise ValueError(f"host_id {host_id} out of 8-bit range")
    if not 0 <= motor_id <= 0xFF:
        raise ValueError(f"motor_id {motor_id} out of 8-bit range")
    return (comm_type << 24) | (host_id << 8) | motor_id


def open_bus(iface: str) -> socket.socket:
    s = socket.socket(socket.AF_CAN, socket.SOCK_RAW, socket.CAN_RAW)
    s.bind((iface,))
    # Linux delivers a copy of our own TX frames back to our RAW socket by
    # default.  Disable that so we don't see our read requests as "replies".
    # Linux/can.h: SOL_CAN_RAW=101, CAN_RAW_RECV_OWN_MSGS=4.
    SOL_CAN_RAW = 101
    CAN_RAW_RECV_OWN_MSGS = 4
    try:
        s.setsockopt(SOL_CAN_RAW, CAN_RAW_RECV_OWN_MSGS, 0)
    except OSError as exc:
        print(f"WARNING: setsockopt(CAN_RAW_RECV_OWN_MSGS, 0) failed: {exc}")
        print("         Falling back to content-based TX filtering.")
    s.settimeout(SOCKET_TIMEOUT_S)
    return s


VERBOSE = False


def _fmt_frame(can_id: int, data: bytes) -> str:
    raw_id = can_id & 0x1FFFFFFF
    return f"{raw_id:08X} [{len(data)}] " + " ".join(f"{b:02X}" for b in data)


def send_frame(sock: socket.socket, comm_type: int, host_id: int,
               motor_id: int, data: bytes) -> None:
    if len(data) > 8:
        raise ValueError("CAN data frame max 8 bytes")
    data = data.ljust(8, b"\x00")
    can_id = arb_id(comm_type, host_id, motor_id) | CAN_EFF_FLAG
    frame = struct.pack("=IB3x8s", can_id, 8, data)
    sock.send(frame)
    if VERBOSE:
        print(f"    TX {_fmt_frame(can_id, data)}")


def recv_frame(sock: socket.socket):
    frame = sock.recv(16)
    can_id, dlc = struct.unpack("=IB3x", frame[:8])
    data = frame[8:8 + dlc]
    comm_type = (can_id >> 24) & 0x1F
    if VERBOSE:
        print(f"    RX {_fmt_frame(can_id, data)} (type={comm_type})")
    return comm_type, can_id, data


# --- Param read/write --------------------------------------------------------

def read_param_raw(sock: socket.socket, host_id: int, motor_id: int,
                   index: int, timeout_s: float = 0.5) -> bytes | None:
    """Send a type-17 read, wait for the matching reply, return the 4 value
    bytes (little-endian).  None on timeout or mismatch.

    Motor replies invert the arbitration ID: our TX has bits 23..16 = host_id,
    bits 7..0 = motor_id.  A legitimate reply from the motor is the mirror
    image: bits 23..16 = motor_id (as source), bits 7..0 = host_id (as dest).
    We use that to distinguish replies from self-loopback if the kernel ignores
    our CAN_RAW_RECV_OWN_MSGS setting.
    """
    req = struct.pack("<HHI", index, 0, 0)  # idx, pad, zero
    tx_id = arb_id(COMM_TYPE_READ_PARAM, host_id, motor_id)
    expected_reply_id = arb_id(COMM_TYPE_READ_PARAM, motor_id, host_id)
    send_frame(sock, COMM_TYPE_READ_PARAM, host_id, motor_id, req)

    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            comm_type, can_id, data = recv_frame(sock)
        except (socket.timeout, TimeoutError):
            return None
        raw_id = can_id & 0x1FFFFFFF
        if raw_id == tx_id:
            # Self-loopback -- ignore.  This is the frame we just sent.
            continue
        if comm_type != COMM_TYPE_READ_PARAM:
            continue
        if raw_id != expected_reply_id:
            # Wrong source/dest; not a reply to our request.
            continue
        if len(data) < 8:
            continue
        reply_index = struct.unpack("<H", data[:2])[0]
        if reply_index != index:
            continue
        return bytes(data[4:8])
    return None


def read_float(sock, host_id, motor_id, index):
    raw = read_param_raw(sock, host_id, motor_id, index)
    return struct.unpack("<f", raw)[0] if raw else None


def read_u32(sock, host_id, motor_id, index):
    raw = read_param_raw(sock, host_id, motor_id, index)
    return struct.unpack("<I", raw)[0] if raw else None


# --- Commands ----------------------------------------------------------------

def cmd_stop(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_STOP, host_id, motor_id, b"\x00")


def cmd_set_zero(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_SET_ZERO, host_id, motor_id, b"\x01")


def cmd_save_params(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_SAVE_PARAMS, host_id, motor_id, b"")


# --- Top-level flow ----------------------------------------------------------

def dump_state(sock, host_id, motor_id, label):
    print(f"\n--- state: {label} ---")
    fields = [
        ("MechOffset (0x2005)",    PARAM_MECH_OFFSET,    "float"),
        ("mechPos    (0x3016)",    PARAM_MECH_POS,       "float"),
        ("mechVel    (0x3017)",    PARAM_MECH_VEL,       "float"),
        ("faultSta   (0x3022)",    PARAM_FAULT_STA,      "u32"),
        ("CAN_ID     (0x2009)",    PARAM_CAN_ID,         "u32"),  # uint8 but read as u32 low byte
    ]
    out = {}
    for name, idx, kind in fields:
        if kind == "float":
            v = read_float(sock, host_id, motor_id, idx)
            print(f"  {name} = {v}")
        else:
            v = read_u32(sock, host_id, motor_id, idx)
            print(f"  {name} = {v}")
        out[name] = v
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--iface", default="can0")
    ap.add_argument("--motor-id", type=lambda s: int(s, 0), default=0x08)
    ap.add_argument("--host-id", type=lambda s: int(s, 0), default=0xFD)
    ap.add_argument("--read-only", action="store_true",
                    help="Read and print state, send no admin frames.")
    ap.add_argument("--set-zero", action="store_true",
                    help="Issue type-6 Set Mechanical Zero.")
    ap.add_argument("--save", action="store_true",
                    help="Issue type-22 Save Parameters after any writes.")
    ap.add_argument("--verbose", "-v", action="store_true",
                    help="Print every TX/RX CAN frame in hex.")
    args = ap.parse_args()

    global VERBOSE
    VERBOSE = args.verbose

    sock = open_bus(args.iface)
    print(f"bound {args.iface}, talking to motor 0x{args.motor_id:02X} "
          f"as host 0x{args.host_id:02X}")

    state0 = dump_state(sock, args.host_id, args.motor_id, "initial")

    if state0["faultSta   (0x3022)"] not in (0, None) and state0["faultSta   (0x3022)"] != 0:
        print(f"\nABORT: faultSta = {state0['faultSta   (0x3022)']} (non-zero). "
              "Investigate before commissioning.")
        return 2

    mech_vel = state0["mechVel    (0x3017)"]
    if mech_vel is not None and abs(mech_vel) > POWER_SANITY_MAX_VEL_RAD_S:
        print(f"\nABORT: mechVel = {mech_vel} rad/s > {POWER_SANITY_MAX_VEL_RAD_S}. "
              "Shaft is spinning; hold it still and retry.")
        return 2

    if args.read_only:
        print("\n--read-only was set; no admin frames sent.")
        return 0

    if args.set_zero:
        print("\n>>> Stopping motor (comm type 4)")
        cmd_stop(sock, args.host_id, args.motor_id)
        time.sleep(0.1)

        print(">>> Sending Set Mechanical Zero (comm type 6, byte[0]=1)")
        cmd_set_zero(sock, args.host_id, args.motor_id)
        time.sleep(0.2)

        dump_state(sock, args.host_id, args.motor_id, "after Set Zero (RAM)")

    if args.save:
        print("\n>>> Sending Save Parameters to Flash (comm type 22)")
        cmd_save_params(sock, args.host_id, args.motor_id)
        time.sleep(SAVE_SETTLE_S)

        dump_state(sock, args.host_id, args.motor_id, "after Save to Flash")
        print("\nNext step: POWER-CYCLE the motor, re-run this script with "
              "--read-only, and confirm MechOffset persisted.")

    return 0


if __name__ == "__main__":
    sys.exit(main())
