# DEPRECATED 2026-04-17. See ADR-0003.
# Canonical implementation: src/driver (Rust), CLI: `cargo run --bin bench_tool`.
# These scripts are frozen pending deletion once bench_tool passes on
# shoulder_actuator_a. Do NOT add new features here.

"""Shared CAN framing + parameter I/O for RobStride RS03 bench tools.

This module is the single source of truth for the RS03 CAN protocol encode/
decode used by the commissioning scripts under tools/robstride/.  It is NOT
the driver.  The real driver (src/driver) will own its own, more rigorous
implementation (proper timeouts, ACK tracking, async I/O, error types).

Scope kept here:
  - 29-bit arbitration ID build/decode
  - SocketCAN raw socket open, with self-loopback suppressed
  - send_frame / recv_frame primitives with a single verbose-logging toggle
  - type-17 single-parameter read with correct reply-status-bit handling
  - type-18 single-parameter write helpers (float / uint8 / uint32)
  - protocol constants (COMM_TYPE_*, PARAM_*, CAN_EFF_FLAG, ...)

All references below are to docs/decisions/0002-rs03-protocol-spec.md and,
through it, docs/vendor/rs03-user-manual-260112.pdf §4.1.
"""

from __future__ import annotations

import socket
import struct
import time

# --- Communication types (ADR-0002 §"Communication types") -----------------

COMM_TYPE_GET_DEVICE_ID = 0x00
COMM_TYPE_OPERATION_CTRL = 0x01  # MIT-style pos+vel+kp+kd+tff frame
COMM_TYPE_MOTOR_FEEDBACK = 0x02  # reply frame while motor is active
COMM_TYPE_ENABLE = 0x03
COMM_TYPE_STOP = 0x04            # byte[0]=1 clears fault
COMM_TYPE_SET_ZERO = 0x06        # byte[0]=1
COMM_TYPE_SET_CAN_ID = 0x07
COMM_TYPE_READ_PARAM = 0x11      # type-17 single parameter read (0x70xx only)
COMM_TYPE_WRITE_PARAM = 0x12     # type-18 single parameter write (RAM)
COMM_TYPE_FAULT_FEEDBACK = 0x15
COMM_TYPE_SAVE_PARAMS = 0x16     # type-22 persist RAM writes to flash

# --- Parameter indices (ADR-0002 §"Safety-critical parameters") ------------
#
# IMPORTANT: type-17 (single parameter read) can ONLY read indices in the
# 0x70xx namespace (see vendor manual §4.1.14 "Read and write a single
# parameter list").  The 0x20xx stored-config and 0x30xx runtime-observables
# namespaces visible in Motor Studio are NOT addressable via type-17 -- the
# motor will reply with bit 16 of the arb ID set ("read failed") and zero
# value bytes.

PARAM_RUN_MODE = 0x7005        # uint8:  0=MIT, 1=PP, 2=velocity, 3=current, 5=CSP
PARAM_IQ_REF = 0x7006          # float:  current-mode setpoint, A
PARAM_SPD_REF = 0x700A         # float:  velocity-mode setpoint, rad/s
PARAM_LIMIT_TORQUE = 0x700B    # float:  hard torque clamp, Nm
PARAM_LOC_REF = 0x7016         # float:  position-mode setpoint, rad
PARAM_LIMIT_SPD = 0x7017       # float:  hard speed clamp, rad/s
PARAM_LIMIT_CUR = 0x7018       # float:  hard phase-current clamp, A
PARAM_MECH_POS = 0x7019        # float:  mechPos, rad (shadow of 0x3016)
PARAM_IQF = 0x701A             # float:  filtered q-axis current, A
PARAM_MECH_VEL = 0x701B        # float:  mechVel, rad/s (shadow of 0x3017)
PARAM_VBUS = 0x701C            # float:  bus voltage, V
PARAM_ACC_RAD = 0x7022         # float:  velocity-mode accel, rad/s^2
PARAM_VEL_MAX = 0x7024         # float:  PP-mode max speed, rad/s
PARAM_ACC_SET = 0x7025         # float:  PP-mode accel, rad/s^2
PARAM_CAN_TIMEOUT = 0x7028     # uint32: canTimeout, 20000 counts = 1 s
PARAM_ZERO_STA = 0x7029        # uint8:  0 = 0..2pi reporting, 1 = -pi..pi
PARAM_DAMPER = 0x702A          # uint8:  1 = disable post-power-off damping
PARAM_ADD_OFFSET = 0x702B      # float:  add_offset, rad

# --- Reply status flag (bit 16 of reply arb ID, §4.1.6) --------------------

READ_STATUS_OK = 0x00
READ_STATUS_FAIL = 0x01

# --- SocketCAN ------------------------------------------------------------

CAN_EFF_FLAG = 0x80000000
DEFAULT_SOCKET_TIMEOUT_S = 0.5


# --- Verbose logging toggle ------------------------------------------------
#
# Prefer set_verbose() over reaching into the module.  Kept as a module-level
# flag rather than threaded through every call because these scripts are
# single-threaded, short-lived, and value debuggability over purity.

_VERBOSE = False


def set_verbose(on: bool) -> None:
    global _VERBOSE
    _VERBOSE = bool(on)


def is_verbose() -> bool:
    return _VERBOSE


# --- CAN framing -----------------------------------------------------------

def arb_id(comm_type: int, host_id: int, motor_id: int) -> int:
    """Build the 29-bit arbitration ID per ADR-0002."""
    if not 0 <= comm_type <= 0x1F:
        raise ValueError(f"comm_type {comm_type} out of 5-bit range")
    if not 0 <= host_id <= 0xFF:
        raise ValueError(f"host_id {host_id} out of 8-bit range")
    if not 0 <= motor_id <= 0xFF:
        raise ValueError(f"motor_id {motor_id} out of 8-bit range")
    return (comm_type << 24) | (host_id << 8) | motor_id


def open_bus(iface: str, timeout_s: float = DEFAULT_SOCKET_TIMEOUT_S) -> socket.socket:
    """Open a raw SocketCAN socket bound to iface, with self-loopback off."""
    s = socket.socket(socket.AF_CAN, socket.SOCK_RAW, socket.CAN_RAW)
    s.bind((iface,))
    # Linux delivers a copy of our own TX frames back to our RAW socket by
    # default.  Disable that so we don't see our own requests as "replies".
    # From <linux/can.h>: SOL_CAN_RAW=101, CAN_RAW_RECV_OWN_MSGS=4.
    SOL_CAN_RAW = 101
    CAN_RAW_RECV_OWN_MSGS = 4
    try:
        s.setsockopt(SOL_CAN_RAW, CAN_RAW_RECV_OWN_MSGS, 0)
    except OSError as exc:
        print(f"WARNING: setsockopt(CAN_RAW_RECV_OWN_MSGS, 0) failed: {exc}")
        print("         Falling back to content-based TX filtering.")
    s.settimeout(timeout_s)
    return s


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
    if _VERBOSE:
        print(f"    TX {_fmt_frame(can_id, data)}")


def send_raw_frame(sock: socket.socket, raw_arb_id: int, data: bytes) -> None:
    """Escape hatch for frames whose data-area-2 is not just the host_id.

    Operation-control frames (type 1) pack torque-ff into bits 23..16 of the
    arbitration ID, which doesn't fit the (comm_type, host, motor) shape of
    send_frame().  Callers that need this must build the 29-bit ID themselves.
    """
    if len(data) > 8:
        raise ValueError("CAN data frame max 8 bytes")
    data = data.ljust(8, b"\x00")
    can_id = (raw_arb_id & 0x1FFFFFFF) | CAN_EFF_FLAG
    frame = struct.pack("=IB3x8s", can_id, 8, data)
    sock.send(frame)
    if _VERBOSE:
        print(f"    TX {_fmt_frame(can_id, data)}")


def recv_frame(sock: socket.socket):
    """Blocking recv of a single frame, honoring the socket's timeout.

    Returns (comm_type, full_can_id_including_eff_flag, data).  Raises
    socket.timeout / TimeoutError on timeout -- callers decide what to do.
    """
    frame = sock.recv(16)
    can_id, dlc = struct.unpack("=IB3x", frame[:8])
    data = frame[8:8 + dlc]
    comm_type = (can_id >> 24) & 0x1F
    if _VERBOSE:
        print(f"    RX {_fmt_frame(can_id, data)} (type={comm_type})")
    return comm_type, can_id, data


# --- Parameter read (type 17) ---------------------------------------------

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
            continue  # self-loopback guard (belt + braces)
        if comm_type != COMM_TYPE_READ_PARAM:
            continue
        reply_status = (raw_id >> 16) & 0xFF
        reply_motor = (raw_id >> 8) & 0xFF
        reply_host = raw_id & 0xFF
        if reply_motor != motor_id or reply_host != host_id:
            continue
        if len(data) < 8:
            continue
        reply_index = struct.unpack("<H", data[:2])[0]
        if reply_index != index:
            continue
        if reply_status == READ_STATUS_FAIL:
            if _VERBOSE:
                print(f"    -> motor rejected read of index 0x{index:04X} "
                      f"(status byte = 0x{reply_status:02X}). "
                      "Is this index in the 0x70xx namespace?")
            return None
        if reply_status != READ_STATUS_OK:
            if _VERBOSE:
                print(f"    -> reply status byte = 0x{reply_status:02X} "
                      f"(expected 0x00 or 0x01); treating as failure.")
            return None
        return bytes(data[4:8])
    return None


def read_float(sock, host_id, motor_id, index, timeout_s: float = 0.5):
    raw = read_param_raw(sock, host_id, motor_id, index, timeout_s)
    return struct.unpack("<f", raw)[0] if raw else None


def read_u32(sock, host_id, motor_id, index, timeout_s: float = 0.5):
    raw = read_param_raw(sock, host_id, motor_id, index, timeout_s)
    return struct.unpack("<I", raw)[0] if raw else None


def read_u8(sock, host_id, motor_id, index, timeout_s: float = 0.5):
    raw = read_param_raw(sock, host_id, motor_id, index, timeout_s)
    return raw[0] if raw else None


# --- Parameter write (type 18) --------------------------------------------
#
# ADR-0002 §"Parameter-write frame layout":
#   ID:   [type=0x12][host_id][motor_can_id]
#   data: [idx_lo][idx_hi][0x00][0x00][val0][val1][val2][val3]
#
# Writes are RAM-only.  To persist, caller must follow with cmd_save_params().

def write_param_float(sock, host_id, motor_id, index: int, value: float) -> None:
    payload = struct.pack("<HHf", index, 0, float(value))
    send_frame(sock, COMM_TYPE_WRITE_PARAM, host_id, motor_id, payload)


def write_param_u8(sock, host_id, motor_id, index: int, value: int) -> None:
    if not 0 <= int(value) <= 0xFF:
        raise ValueError(f"u8 value {value} out of range")
    payload = struct.pack("<HHBxxx", index, 0, int(value))
    send_frame(sock, COMM_TYPE_WRITE_PARAM, host_id, motor_id, payload)


def write_param_u32(sock, host_id, motor_id, index: int, value: int) -> None:
    if not 0 <= int(value) <= 0xFFFFFFFF:
        raise ValueError(f"u32 value {value} out of range")
    payload = struct.pack("<HHI", index, 0, int(value))
    send_frame(sock, COMM_TYPE_WRITE_PARAM, host_id, motor_id, payload)


# --- Admin frames ----------------------------------------------------------

def cmd_stop(sock, host_id, motor_id, clear_fault: bool = False) -> None:
    """type-4 stop.  byte[0]=1 also clears fault; default is just stop."""
    send_frame(sock, COMM_TYPE_STOP, host_id, motor_id,
               bytes([1 if clear_fault else 0]))


def cmd_enable(sock, host_id, motor_id) -> None:
    """type-3 enable motor run.  Data bytes are don't-care (§4.1.4)."""
    send_frame(sock, COMM_TYPE_ENABLE, host_id, motor_id, b"")


def cmd_set_zero(sock, host_id, motor_id) -> None:
    send_frame(sock, COMM_TYPE_SET_ZERO, host_id, motor_id, b"\x01")


def cmd_save_params(sock, host_id, motor_id) -> None:
    send_frame(sock, COMM_TYPE_SAVE_PARAMS, host_id, motor_id, b"")


# --- Motor feedback frame (type 2) decode ---------------------------------
#
# Layout per vendor manual §4.1.3 (to be confirmed empirically on our FW):
#   Reply arb ID: [type=0x02][motor_can_id(8)][fault_bits(6)][mode(2)][host]
#       bits 28..24 : 0x02
#       bits 23..16 : source motor CAN_ID
#       bits 15..8  : fault/mode status
#       bits  7..0  : host CAN_ID (dest)
#   Data (big-endian within each 16-bit field):
#       bytes 0..1 : mechPos raw, 0..65535 maps to -4pi..+4pi rad
#       bytes 2..3 : mechVel raw, 0..65535 maps to -20..+20 rad/s
#       bytes 4..5 : torque raw,  0..65535 maps to -60..+60 Nm
#       bytes 6..7 : MOS temperature, raw / 10 = degC
#
# Keep this decoder conservative and flag inconsistencies loudly -- the first
# time we see a real type-2 frame in the smoke test we will sanity-check
# mechPos against what type-17 reads from 0x7019, and then pin the layout
# into ADR-0002.

def _u16_to_range(raw: int, lo: float, hi: float) -> float:
    raw &= 0xFFFF
    return lo + (raw / 65535.0) * (hi - lo)


def decode_motor_feedback(can_id: int, data: bytes) -> dict:
    """Best-effort decode of a type-2 motor feedback frame.

    Returns a dict with pos_rad, vel_rad_s, torque_nm, temp_c, and the
    raw status bits extracted from the arbitration ID.  Caller is
    responsible for having verified comm_type == 2 before calling.
    """
    if len(data) < 8:
        raise ValueError(f"motor feedback frame too short: {len(data)} bytes")
    raw_id = can_id & 0x1FFFFFFF
    src_motor = (raw_id >> 16) & 0xFF
    status = (raw_id >> 8) & 0xFF  # fault+mode mixed; decode per ADR when confirmed
    dest_host = raw_id & 0xFF
    pos_raw = struct.unpack(">H", data[0:2])[0]
    vel_raw = struct.unpack(">H", data[2:4])[0]
    tor_raw = struct.unpack(">H", data[4:6])[0]
    temp_raw = struct.unpack(">H", data[6:8])[0]
    return {
        "src_motor": src_motor,
        "dest_host": dest_host,
        "status_byte": status,
        "pos_rad": _u16_to_range(pos_raw, -12.566370614, 12.566370614),  # -4pi..+4pi
        "vel_rad_s": _u16_to_range(vel_raw, -20.0, 20.0),
        "torque_nm": _u16_to_range(tor_raw, -60.0, 60.0),
        "temp_c": temp_raw / 10.0,
        "raw": {
            "pos": pos_raw, "vel": vel_raw, "torque": tor_raw, "temp": temp_raw,
        },
    }
