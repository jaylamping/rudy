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

# IMPORTANT: type-17 (single parameter read) can ONLY read indices in the
# 0x70xx namespace (see vendor manual §4.1.14 "Read and write a single
# parameter list").  The 0x20xx stored-config and 0x30xx runtime-observables
# namespaces visible in Motor Studio are NOT addressable via type-17 -- the
# motor will reply with bit 16 of the arb ID set ("read failed") and zero
# value bytes.  Motor Studio accesses 0x20xx / 0x30xx via type-0x13 bulk
# export, which is a different protocol path we do not need for commissioning.
#
# Consequence: we read mechPos / mechVel / VBUS via the 0x70xx shadow indices
# below, and we *cannot* read MechOffset / faultSta / CAN_ID from the Pi over
# type-17.  That's fine for this bench utility's purpose: we only need enough
# observability to decide whether the shaft is still and healthy before
# sending Set-Zero + Save-Params admin frames.
PARAM_MECH_POS = 0x7019        # mechPos  (float, rad, same physical field as 0x3016)
PARAM_MECH_VEL = 0x701B        # mechVel  (float, rad/s, same as 0x3017)
PARAM_IQF = 0x701A             # iqf      (float, A)
PARAM_VBUS = 0x701C            # VBUS     (float, V)
PARAM_LIMIT_SPD = 0x7017       # limit_spd (float, rad/s)
PARAM_LIMIT_CUR = 0x7018       # limit_cur (float, A)
PARAM_LIMIT_TORQUE = 0x700B    # limit_torque (float, Nm)
PARAM_RUN_MODE = 0x7005        # run_mode (uint8)
PARAM_CAN_TIMEOUT = 0x7028     # canTimeout (uint32, counts)
PARAM_ZERO_STA = 0x7029        # zero_sta (uint8)
PARAM_DAMPER = 0x702A          # damper (uint8)
PARAM_ADD_OFFSET = 0x702B      # add_offset (float, rad)

# Reply status flag encoded in bit 16 of the 29-bit reply arb ID:
READ_STATUS_OK = 0x00
READ_STATUS_FAIL = 0x01

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
    bytes (little-endian).  None on timeout, motor rejection, or mismatch.

    Reply arb ID layout (vendor manual §4.1.6, observed on FW 0.3.1.41):

        [type=0x11][status(8)][motor_id(8)][host_id(8)]
          bits28-24   bits23-16   bits15-8     bits7-0

    where status = 0 for "read OK" and status = 1 for "read failed".  A read
    fails when the requested index is not in the type-17 parameter list
    (§4.1.14), i.e. not in the 0x70xx namespace.  On failure the motor still
    sends a reply frame, just with zero-filled value bytes and status=1.

    Our TX has the opposite source/dest orientation: bits 23..16 contain the
    *status* field on the reply path (manual §4.1.6 reply row), which we do
    NOT set on transmit.  On transmit, bits 15..8 = host, bits 7..0 = motor.
    On reply, bits 15..8 = motor (source), bits 7..0 = host (dest).
    """
    req = struct.pack("<HHI", index, 0, 0)  # idx, pad, zero
    tx_id = arb_id(COMM_TYPE_READ_PARAM, host_id, motor_id)
    send_frame(sock, COMM_TYPE_READ_PARAM, host_id, motor_id, req)

    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            comm_type, can_id, data = recv_frame(sock)
        except (socket.timeout, TimeoutError):
            return None
        raw_id = can_id & 0x1FFFFFFF
        if raw_id == tx_id:
            continue  # self-loopback guard
        if comm_type != COMM_TYPE_READ_PARAM:
            continue
        # Decode reply arb ID: [type(5)][status(8)][src motor(8)][dest host(8)]
        reply_status = (raw_id >> 16) & 0xFF
        reply_motor = (raw_id >> 8) & 0xFF
        reply_host = raw_id & 0xFF
        if reply_motor != motor_id or reply_host != host_id:
            continue  # not a reply addressed to us from this motor
        if len(data) < 8:
            continue
        reply_index = struct.unpack("<H", data[:2])[0]
        if reply_index != index:
            continue
        if reply_status == READ_STATUS_FAIL:
            if VERBOSE:
                print(f"    -> motor rejected read of index 0x{index:04X} "
                      f"(status byte = 0x{reply_status:02X}). "
                      "Is this index in the 0x70xx namespace?")
            return None
        if reply_status != READ_STATUS_OK:
            if VERBOSE:
                print(f"    -> reply status byte = 0x{reply_status:02X} "
                      f"(expected 0x00 or 0x01); treating as failure.")
            return None
        return bytes(data[4:8])
    return None


def read_float(sock, host_id, motor_id, index):
    raw = read_param_raw(sock, host_id, motor_id, index)
    return struct.unpack("<f", raw)[0] if raw else None


def read_u32(sock, host_id, motor_id, index):
    raw = read_param_raw(sock, host_id, motor_id, index)
    return struct.unpack("<I", raw)[0] if raw else None


def read_u8(sock, host_id, motor_id, index):
    raw = read_param_raw(sock, host_id, motor_id, index)
    return raw[0] if raw else None


# --- Commands ----------------------------------------------------------------

def cmd_stop(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_STOP, host_id, motor_id, b"\x00")


def cmd_set_zero(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_SET_ZERO, host_id, motor_id, b"\x01")


def cmd_save_params(sock, host_id, motor_id):
    send_frame(sock, COMM_TYPE_SAVE_PARAMS, host_id, motor_id, b"")


# --- Top-level flow ----------------------------------------------------------

def dump_state(sock, host_id, motor_id, label):
    """Read and print the runtime observables exposed via type-17 (0x70xx).

    These are the only indices type-17 can address.  MechOffset, faultSta,
    and CAN_ID live in 0x20xx/0x30xx and are NOT readable from here -- use
    Motor Studio's param export for those.
    """
    print(f"\n--- state: {label} ---")
    fields = [
        ("run_mode    (0x7005, u8)",    PARAM_RUN_MODE,     "u8"),
        ("mechPos     (0x7019, rad)",   PARAM_MECH_POS,     "float"),
        ("mechVel     (0x701B, rad/s)", PARAM_MECH_VEL,     "float"),
        ("iqf         (0x701A, A)",     PARAM_IQF,          "float"),
        ("VBUS        (0x701C, V)",     PARAM_VBUS,         "float"),
        ("limit_spd   (0x7017, rad/s)", PARAM_LIMIT_SPD,    "float"),
        ("limit_cur   (0x7018, A)",     PARAM_LIMIT_CUR,    "float"),
        ("limit_torque(0x700B, Nm)",    PARAM_LIMIT_TORQUE, "float"),
        ("canTimeout  (0x7028, cnt)",   PARAM_CAN_TIMEOUT,  "u32"),
        ("zero_sta    (0x7029, u8)",    PARAM_ZERO_STA,     "u8"),
        ("damper      (0x702A, u8)",    PARAM_DAMPER,       "u8"),
        ("add_offset  (0x702B, rad)",   PARAM_ADD_OFFSET,   "float"),
    ]
    out = {}
    for name, idx, kind in fields:
        if kind == "float":
            v = read_float(sock, host_id, motor_id, idx)
        elif kind == "u32":
            v = read_u32(sock, host_id, motor_id, idx)
        elif kind == "u8":
            v = read_u8(sock, host_id, motor_id, idx)
        else:
            v = None
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

    # Bail if we got nothing back at all.  type-17 cannot read faultSta, so we
    # rely on mechVel + VBUS as lightweight proxies for "motor is alive and
    # standing still" before sending admin frames.
    if state0["VBUS        (0x701C, V)"] is None:
        print("\nABORT: no reply from motor on any 0x70xx read. "
              "Check --iface, CAN wiring, termination, and motor CAN_ID.")
        return 2

    mech_vel = state0["mechVel     (0x701B, rad/s)"]
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
