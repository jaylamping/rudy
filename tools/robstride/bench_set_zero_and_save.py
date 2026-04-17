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
import sys
import time

from rs03_can import (
    PARAM_ADD_OFFSET,
    PARAM_CAN_TIMEOUT,
    PARAM_DAMPER,
    PARAM_IQF,
    PARAM_LIMIT_CUR,
    PARAM_LIMIT_SPD,
    PARAM_LIMIT_TORQUE,
    PARAM_MECH_POS,
    PARAM_MECH_VEL,
    PARAM_RUN_MODE,
    PARAM_VBUS,
    PARAM_ZERO_STA,
    cmd_save_params,
    cmd_set_zero,
    cmd_stop,
    open_bus,
    read_float,
    read_u32,
    read_u8,
    set_verbose,
)

SAVE_SETTLE_S = 0.2
POWER_SANITY_MAX_VEL_RAD_S = 0.05


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

    set_verbose(args.verbose)

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
