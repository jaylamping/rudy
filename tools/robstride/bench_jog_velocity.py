#!/usr/bin/env python3
# DEPRECATED 2026-04-17. See ADR-0003.
# Canonical implementation: ros/src/driver (Rust), CLI: cargo run --bin bench_tool.
# These scripts are frozen pending deletion once bench_tool passes on
# shoulder_actuator_a. Do NOT add new features here.

"""Bench utility: first jog from the Pi in velocity mode (run_mode=2).

Layers software caps on top of firmware limit_spd / limit_cur. Dry-run by
default; pass --go to command motion. Use --test-overlimit to prove firmware
limit_spd clamps (Step 9 gate).

Usage:

    cd ~/rudy && sudo python3 ./tools/robstride/bench_jog_velocity.py \\
        --iface can1 --motor-id 0x08 --host-id 0xFD \\
        --target-vel 0.2 --duration 2.0 --verbose

    cd ~/rudy && sudo python3 ./tools/robstride/bench_jog_velocity.py \\
        --iface can1 --motor-id 0x08 --host-id 0xFD \\
        --target-vel 0.2 --duration 2.0 --go --verbose

    cd ~/rudy && sudo python3 ./tools/robstride/bench_jog_velocity.py \\
        --iface can1 --motor-id 0x08 --host-id 0xFD --go --test-overlimit -v

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

# Software caps (CLI cannot override)
MAX_TARGET_VEL_RAD_S = 0.5
MAX_DURATION_S = 3.0
WATCHDOG_VEL_RAD_S = 1.0
RAMP_S = 0.5
TICK_S = 0.05
POLL_RECV_TIMEOUT_S = 0.02
MIN_VBUS_V = 20.0
STILL_VEL_GATE_RAD_S = 0.05
LIMIT_SPD_EXPECTED = 3.0
LIMIT_SPD_TOL = 0.05

# Overlimit test
OVERLIMIT_SPD_REF = 20.0
OVERLIMIT_HOLD_S = 0.5
OVERLIMIT_FAIL_ABOVE = 3.5
OVERLIMIT_OK_LO = 2.5
OVERLIMIT_OK_HI = 3.2


def dump_minimal(sock, host_id: int, motor_id: int, label: str) -> None:
    print(f"\n--- {label} ---")
    vbus = read_float(sock, host_id, motor_id, PARAM_VBUS)
    mech_vel = read_float(sock, host_id, motor_id, PARAM_MECH_VEL)
    limit_spd = read_float(sock, host_id, motor_id, PARAM_LIMIT_SPD)
    run_mode = read_u8(sock, host_id, motor_id, PARAM_RUN_MODE)
    print(f"  VBUS = {vbus} V")
    print(f"  mechVel   = {mech_vel} rad/s")
    print(f"  limit_spd = {limit_spd} rad/s")
    print(f"  run_mode  = {run_mode}")


def defang_motor(sock, host_id: int, motor_id: int) -> None:
    cmd_stop(sock, host_id, motor_id, clear_fault=False)
    write_param_u8(sock, host_id, motor_id, PARAM_RUN_MODE, 0)
    write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, 0.0)
    time.sleep(0.05)


def drain_feedback(sock, host_id: int, motor_id: int,
                   last_vel: float | None, last_time: float) -> tuple[float | None, float]:
    """Non-blocking-ish drain of type-2 frames; update last vel if ours."""
    sock.settimeout(POLL_RECV_TIMEOUT_S)
    while True:
        try:
            comm_type, can_id, data = recv_frame(sock)
        except (socket.timeout, TimeoutError):
            break
        if comm_type != COMM_TYPE_MOTOR_FEEDBACK:
            continue
        raw_id = can_id & 0x1FFFFFFF
        src = (raw_id >> 16) & 0xFF
        dst = raw_id & 0xFF
        if src != motor_id or dst != host_id:
            continue
        try:
            dec = decode_motor_feedback(can_id, data)
        except ValueError:
            continue
        last_vel = dec["vel_rad_s"]
        last_time = time.monotonic()
    return last_vel, last_time


def read_mech_vel_prefer_fb(sock, host_id: int, motor_id: int,
                            last_vel: float | None, last_time: float,
                            fb_max_age_s: float = 0.1) -> tuple[float | None, float | None, float]:
    last_vel, last_time = drain_feedback(sock, host_id, motor_id, last_vel, last_time)
    now = time.monotonic()
    if last_vel is not None and (now - last_time) <= fb_max_age_s:
        return last_vel, last_vel, last_time
    v = read_float(sock, host_id, motor_id, PARAM_MECH_VEL, timeout_s=0.25)
    return v, last_vel, last_time


def sanity_pre_jog(sock, host_id: int, motor_id: int) -> int:
    vbus = read_float(sock, host_id, motor_id, PARAM_VBUS)
    if vbus is None:
        print("FAIL: no reply (VBUS).")
        return 2
    if vbus < MIN_VBUS_V:
        print(f"FAIL: VBUS {vbus} V too low.")
        return 2
    mech_vel = read_float(sock, host_id, motor_id, PARAM_MECH_VEL)
    if mech_vel is not None and abs(mech_vel) > STILL_VEL_GATE_RAD_S:
        print(f"FAIL: shaft moving: mechVel = {mech_vel}")
        return 2
    lim = read_float(sock, host_id, motor_id, PARAM_LIMIT_SPD)
    if lim is None:
        print("FAIL: could not read limit_spd.")
        return 2
    if abs(lim - LIMIT_SPD_EXPECTED) > LIMIT_SPD_TOL:
        print(f"FAIL: limit_spd = {lim} rad/s (expected {LIMIT_SPD_EXPECTED} ± "
              f"{LIMIT_SPD_TOL}). Refusing jog - limits may have been changed.")
        return 2
    return 0


def run_overlimit(sock, host_id: int, motor_id: int, verbose: bool) -> int:
    print("\n>>> OVERLIMIT TEST: spd_ref = 20 rad/s for "
          f"{OVERLIMIT_HOLD_S:.1f}s (firmware should clamp to ~3 rad/s)")
    cmd_stop(sock, host_id, motor_id, clear_fault=False)
    time.sleep(0.05)
    write_param_u8(sock, host_id, motor_id, PARAM_RUN_MODE, 2)
    time.sleep(0.02)
    write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, 0.0)
    time.sleep(0.02)
    cmd_enable(sock, host_id, motor_id)
    write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, float(OVERLIMIT_SPD_REF))

    t_end = time.monotonic() + OVERLIMIT_HOLD_S
    peak = 0.0
    last_v = None
    last_t = 0.0
    sock.settimeout(POLL_RECV_TIMEOUT_S)
    while time.monotonic() < t_end:
        mv, last_v, last_t = read_mech_vel_prefer_fb(sock, host_id, motor_id,
                                                     last_v, last_t)
        if mv is not None:
            peak = max(peak, abs(mv))
            if abs(mv) > OVERLIMIT_FAIL_ABOVE:
                print(f"FAIL: |mechVel| = {abs(mv)} > {OVERLIMIT_FAIL_ABOVE} - "
                      "limit_spd NOT enforcing.")
                return 4
        if verbose and mv is not None:
            print(f"  overlimit sample |v|={abs(mv):.4f} rad/s (peak {peak:.4f})")
        time.sleep(TICK_S)

    print(f"  peak |mechVel| = {peak:.4f} rad/s")
    if peak < OVERLIMIT_OK_LO or peak > OVERLIMIT_OK_HI:
        print(f"FAIL: expected peak in [{OVERLIMIT_OK_LO}, {OVERLIMIT_OK_HI}] "
              f"rad/s (firmware limit ~{LIMIT_SPD_EXPECTED}).")
        return 5
    print("PASS: overlimit test - firmware clamp looks active.")
    return 0


def run_jog_ramp(sock, host_id: int, motor_id: int,
                 target_vel: float, duration_s: float, verbose: bool) -> int:
    if duration_s < 1.0:
        print("FAIL: duration must be >= 1.0 s (ramp + hold + ramp).")
        return 2
    hold_s = duration_s - 2.0 * RAMP_S
    if hold_s < 0:
        print("FAIL: internal ramp math error.")
        return 2

    print(f"\n>>> JOG: target_vel={target_vel} rad/s, total={duration_s}s "
          f"(ramp {RAMP_S}s + hold {hold_s:.2f}s + ramp {RAMP_S}s)")

    def desired_spd_at(t_elapsed: float) -> float:
        if t_elapsed < RAMP_S:
            return target_vel * (t_elapsed / RAMP_S)
        if t_elapsed < RAMP_S + hold_s:
            return target_vel
        t2 = t_elapsed - RAMP_S - hold_s
        if t2 < RAMP_S:
            return target_vel * (1.0 - t2 / RAMP_S)
        return 0.0

    cmd_stop(sock, host_id, motor_id, clear_fault=False)
    time.sleep(0.05)
    write_param_u8(sock, host_id, motor_id, PARAM_RUN_MODE, 2)
    time.sleep(0.02)
    write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, 0.0)
    time.sleep(0.02)
    cmd_enable(sock, host_id, motor_id)

    t0 = time.monotonic()
    last_v = None
    last_t = 0.0

    while True:
        elapsed = time.monotonic() - t0
        if elapsed >= duration_s:
            break
        sp = desired_spd_at(elapsed)
        write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, float(sp))

        mv, last_v, last_t = read_mech_vel_prefer_fb(sock, host_id, motor_id,
                                                     last_v, last_t)
        if mv is not None and abs(mv) > WATCHDOG_VEL_RAD_S:
            print(f"\nFAIL: watchdog: |mechVel|={abs(mv)} > {WATCHDOG_VEL_RAD_S}")
            return 3
        if verbose:
            mv_s = f"{mv:+.4f}" if mv is not None else "nan"
            print(f"  t={elapsed:5.2f}s  spd_ref={sp:+.4f}  mechVel={mv_s}")

        time.sleep(TICK_S)

    write_param_float(sock, host_id, motor_id, PARAM_SPD_REF, 0.0)
    print(">>> Ramp complete; holding zero setpoint briefly...")
    time.sleep(0.2)
    print("PASS: jog ramp finished.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--iface", default="can1")
    ap.add_argument("--motor-id", type=lambda s: int(s, 0), default=0x08)
    ap.add_argument("--host-id", type=lambda s: int(s, 0), default=0xFD)
    ap.add_argument("--target-vel", type=float, default=0.2)
    ap.add_argument("--duration", type=float, default=2.0)
    ap.add_argument("--go", action="store_true",
                    help="Actually command motion (default is dry-run).")
    ap.add_argument("--test-overlimit", action="store_true",
                    help="After enable, command spd_ref=20 to prove limit_spd clamps.")
    ap.add_argument("--verbose", "-v", action="store_true")
    args = ap.parse_args()

    set_verbose(args.verbose)

    target = max(-MAX_TARGET_VEL_RAD_S, min(MAX_TARGET_VEL_RAD_S, args.target_vel))
    if abs(args.target_vel) > MAX_TARGET_VEL_RAD_S:
        print(f"NOTE: clamped |target-vel| to ±{MAX_TARGET_VEL_RAD_S} rad/s "
              f"(was {args.target_vel})")

    duration = min(MAX_DURATION_S, max(1.0, args.duration))
    if args.duration < 1.0 or args.duration > MAX_DURATION_S:
        print(f"NOTE: clamped duration to [{1.0}, {MAX_DURATION_S}] s ! using {duration}")

    sock = open_bus(args.iface)
    print(f"bound {args.iface}, motor 0x{args.motor_id:02X}, host 0x{args.host_id:02X}")

    dump_minimal(sock, args.host_id, args.motor_id, "pre-check")

    rc = sanity_pre_jog(sock, args.host_id, args.motor_id)
    if rc != 0:
        return rc

    if not args.go:
        print("\nDry-run (--go not set). Would:")
        if args.test_overlimit:
            print(f"  Run OVERLIMIT: spd_ref={OVERLIMIT_SPD_REF} for {OVERLIMIT_HOLD_S}s")
        else:
            print(f"  Velocity jog: target {target} rad/s, duration {duration} s")
        return 0

    rc = 0
    try:
        if args.test_overlimit:
            rc = run_overlimit(sock, args.host_id, args.motor_id, args.verbose)
        else:
            rc = run_jog_ramp(sock, args.host_id, args.motor_id,
                              target, duration, args.verbose)
    finally:
        print("\n>>> Defang: stop + spd_ref=0 + run_mode=0")
        defang_motor(sock, args.host_id, args.motor_id)
        sock.settimeout(0.5)
        dump_minimal(sock, args.host_id, args.motor_id, "post-run")

    return rc


if __name__ == "__main__":
    sys.exit(main())
