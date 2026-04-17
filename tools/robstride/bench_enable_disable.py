#!/usr/bin/env python3
# DEPRECATED 2026-04-17. See ADR-0003.
# Canonical implementation: src/driver (Rust), CLI: cargo run --bin bench_tool.
# These scripts are frozen pending deletion once bench_tool passes on
# shoulder_actuator_a. Do NOT add new features here.

"""Bench smoke test: enable motor with velocity setpoint pinned at zero, observe, stop.

Validates type-3 enable, type-4 stop, and type-2 feedback decode before any
motion command. If the shaft moves materially while spd_ref stays 0, something
is wrong - fail loudly.

Dry-run by default. Pass --go to actually send enable.

Usage (Pi, shoulder_actuator_a on Waveshare HAT - Linux iface is often can1):

    cd ~/rudy && sudo python3 ./tools/robstride/bench_enable_disable.py \\
        --iface can1 --motor-id 0x08 --host-id 0xFD --verbose

    cd ~/rudy && sudo python3 ./tools/robstride/bench_enable_disable.py \\
        --iface can1 --motor-id 0x08 --host-id 0xFD --go --verbose

Refs: tools/robstride/commission.md Step 9
      docs/decisions/0002-rs03-protocol-spec.md
"""

from __future__ import annotations

import argparse
import socket
import sys
import time

from rs03_can import (
    COMM_TYPE_MOTOR_FEEDBACK,
    PARAM_LIMIT_SPD,
    PARAM_MECH_VEL,
    PARAM_RUN_MODE,
    PARAM_SPD_REF,
    PARAM_VBUS,
    cmd_enable,
    cmd_stop,
    decode_motor_feedback,
    open_bus,
    read_float,
    read_u8,
    recv_frame,
    set_verbose,
    write_param_float,
    write_param_u8,
)

OBSERVE_S = 1.0
MAX_MECH_VEL_DURING_SMOKE_RAD_S = 0.1
MIN_VBUS_V = 20.0
STILL_VEL_GATE_RAD_S = 0.05
POLL_RECV_TIMEOUT_S = 0.02
TYPE17_SAMPLE_PERIOD_S = 0.1


def dump_minimal(sock, host_id: int, motor_id: int, label: str) -> dict:
    print(f"\n--- {label} ---")
    vbus = read_float(sock, host_id, motor_id, PARAM_VBUS)
    mech_vel = read_float(sock, host_id, motor_id, PARAM_MECH_VEL)
    limit_spd = read_float(sock, host_id, motor_id, PARAM_LIMIT_SPD)
    run_mode_u8 = read_u8(sock, host_id, motor_id, PARAM_RUN_MODE)
    print(f"  VBUS = {vbus} V")
    print(f"  mechVel   = {mech_vel} rad/s")
    print(f"  limit_spd = {limit_spd} rad/s")
    print(f"  run_mode  = {run_mode_u8}")
    return {"VBUS": vbus, "mechVel": mech_vel, "limit_spd": limit_spd,
            "run_mode": run_mode_u8}


def defang_motor(sock, host_id: int, motor_id: int) -> None:
    cmd_stop(sock, host_id, motor_id, clear_fault=False)
    write_param_u8(sock, host_id, motor_id, PARAM_RUN_MODE, 0)
    time.sleep(0.05)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--iface", default="can1")
    ap.add_argument("--motor-id", type=lambda s: int(s, 0), default=0x08)
    ap.add_argument("--host-id", type=lambda s: int(s, 0), default=0xFD)
    ap.add_argument("--go", action="store_true",
                    help="Actually send CAN frames (default is dry-run).")
    ap.add_argument("--verbose", "-v", action="store_true")
    args = ap.parse_args()

    set_verbose(args.verbose)

    sock = open_bus(args.iface)
    print(f"bound {args.iface}, motor 0x{args.motor_id:02X}, host 0x{args.host_id:02X}")

    pre = dump_minimal(sock, args.host_id, args.motor_id, "sanity gate")
    if pre["VBUS"] is None:
        print("\nFAIL: no reply from motor (VBUS read failed).")
        return 2
    if pre["VBUS"] < MIN_VBUS_V:
        print(f"\nFAIL: VBUS {pre['VBUS']} V < {MIN_VBUS_V} V.")
        return 2
    if pre["mechVel"] is not None and abs(pre["mechVel"]) > STILL_VEL_GATE_RAD_S:
        print(f"\nFAIL: shaft already moving: mechVel = {pre['mechVel']} rad/s.")
        return 2

    if not args.go:
        print("\nDry-run only (--go not set). Would:")
        print("  1. write run_mode = 2 (velocity)")
        print("  2. write spd_ref  = 0.0")
        print("  3. send type-3 enable")
        print(f"  4. observe {OBSERVE_S:.1f}s: log type-2 frames, sample mechVel")
        print("  5. send type-4 stop (no fault clear)")
        print("  6. write run_mode = 0 (operation / MIT)")
        return 0

    rc = 0
    peak_vel = 0.0
    fb_count = 0
    try:
        print("\n>>> Pre: stop + velocity mode + spd_ref=0")
        cmd_stop(sock, args.host_id, args.motor_id, clear_fault=False)
        time.sleep(0.05)
        write_param_u8(sock, args.host_id, args.motor_id, PARAM_RUN_MODE, 2)
        time.sleep(0.02)
        write_param_float(sock, args.host_id, args.motor_id, PARAM_SPD_REF, 0.0)
        time.sleep(0.02)

        print(">>> Enable (type 3)")
        cmd_enable(sock, args.host_id, args.motor_id)

        t_end = time.monotonic() + OBSERVE_S
        next_type17 = time.monotonic()
        sock.settimeout(POLL_RECV_TIMEOUT_S)

        while time.monotonic() < t_end:
            now = time.monotonic()

            if now >= next_type17:
                v = read_float(sock, args.host_id, args.motor_id, PARAM_MECH_VEL,
                               timeout_s=0.2)
                if v is not None:
                    peak_vel = max(peak_vel, abs(v))
                    if abs(v) > MAX_MECH_VEL_DURING_SMOKE_RAD_S:
                        print(f"\nFAIL: mechVel |{v}| > "
                              f"{MAX_MECH_VEL_DURING_SMOKE_RAD_S} during enable "
                              "with spd_ref=0.")
                        rc = 3
                        break
                next_type17 = now + TYPE17_SAMPLE_PERIOD_S

            if rc != 0:
                break

            try:
                comm_type, can_id, data = recv_frame(sock)
            except (socket.timeout, TimeoutError):
                continue

            if comm_type != COMM_TYPE_MOTOR_FEEDBACK:
                continue
            raw_id = can_id & 0x1FFFFFFF
            src = (raw_id >> 16) & 0xFF
            dst = raw_id & 0xFF
            if src != args.motor_id or dst != args.host_id:
                continue
            try:
                dec = decode_motor_feedback(can_id, data)
            except ValueError:
                continue
            fb_count += 1
            print(f"  [FB#{fb_count}] type-2  vel~{dec['vel_rad_s']:.4f} rad/s  "
                  f"pos~{dec['pos_rad']:.4f} rad  T~{dec['temp_c']:.1f} °C  "
                  f"status=0x{dec['status_byte']:02X}")

        if rc == 0:
            print(f"\nPASS: peak |mechVel| from type-17 samples = {peak_vel:.6f} rad/s "
                  f"(<{MAX_MECH_VEL_DURING_SMOKE_RAD_S})")
            print(f"       type-2 feedback frames logged: {fb_count}")
    finally:
        print("\n>>> Defang: stop + run_mode=0")
        defang_motor(sock, args.host_id, args.motor_id)
        sock.settimeout(0.5)
        dump_minimal(sock, args.host_id, args.motor_id, "post-run")

    return rc


if __name__ == "__main__":
    sys.exit(main())
