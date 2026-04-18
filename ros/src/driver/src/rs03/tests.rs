//! RS03 bench / commissioning routines as a reusable library API.
//!
//! Originally inlined in `src/bin/bench_tool.rs`; pulled out so:
//!
//!   * `bench_tool` (CLI) gets its routines from here through a `Reporter`
//!     impl that prints to stdout.
//!   * `rudydae` invokes the same routines through a `Reporter` impl that
//!     fans out `TestProgress` frames over WebTransport.
//!
//! Why a `Reporter` trait instead of returning `Vec<Line>`: the bench
//! routines run for seconds, and the operator wants to see each step land
//! the moment it happens — not on completion. The trait inverts control so
//! the caller decides whether to print, broadcast, or accumulate.
//!
//! All routines:
//!
//!   * are blocking (they own the bus for the duration via `&CanBus`),
//!   * honour the `stop` flag for clean Ctrl-C termination,
//!   * apply the same hard caps the Python bench scripts did
//!     (MAX_TARGET_VEL_RAD_S, MAX_DURATION_S, WATCHDOG_VEL_RAD_S, etc.),
//!   * always defang the motor on exit (stop + run_mode=0).

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::rs03::params;
use crate::rs03::session::{
    self, cmd_enable, cmd_save_params, cmd_set_zero, cmd_stop, defang_motor, drain_motor_feedback,
    read_param_f32, read_param_u8, write_param_f32, write_param_u8,
};
use crate::rs03::{comm_type_from_id, decode_motor_feedback, strip_eff_flag, CommType};
use crate::socketcan_bus::CanBus;

// --- Same hard caps as Python bench scripts --------------------------------

pub const MAX_TARGET_VEL_RAD_S: f32 = 0.5;
pub const MAX_DURATION_S: f32 = 3.0;
const WATCHDOG_VEL_RAD_S: f32 = 1.0;
const RAMP_S: f32 = 0.5;
const TICK_S: f32 = 0.05;
const POLL_RECV_TIMEOUT_S: f32 = 0.02;
const MIN_VBUS_V: f32 = 20.0;
const STILL_VEL_GATE_RAD_S: f32 = 0.05;
const POWER_SANITY_MAX_VEL_RAD_S: f32 = 0.05;
const SAVE_SETTLE_S: f32 = 0.2;

const OBSERVE_S: f32 = 1.0;
const MAX_MECH_VEL_DURING_SMOKE_RAD_S: f32 = 0.1;
const TYPE17_SAMPLE_PERIOD_S: f32 = 0.1;

const OVERLIMIT_SPD_REF: f32 = 20.0;
const OVERLIMIT_HOLD_S: f32 = 0.5;
const OVERLIMIT_FAIL_ABOVE: f32 = 3.5;
const OVERLIMIT_OK_LO: f32 = 2.5;
const OVERLIMIT_OK_HI: f32 = 3.2;

const LIMIT_SPD_EXPECTED: f32 = 3.0;
const LIMIT_SPD_TOL: f32 = 0.05;

/// Severity for one progress line.
#[derive(Debug, Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Pass,
    Fail,
}

/// Caller-supplied output sink for one bench routine. The bench routines
/// invoke `report` synchronously between CAN ops; implementations should
/// be cheap (writing into a channel / broadcast::Sender / println!).
pub trait Reporter: Send {
    /// Emit one progress line tagged with a coarse `step` name + severity.
    fn report(&mut self, step: &str, level: Level, message: &str);
}

/// Outcome of a bench routine. Wraps a process-style exit code so the CLI
/// can keep using `std::process::exit(rc)` without translation.
#[derive(Debug, Clone, Copy)]
pub enum RoutineOutcome {
    Pass,
    Fail(i32),
}

impl RoutineOutcome {
    pub fn exit_code(self) -> i32 {
        match self {
            RoutineOutcome::Pass => 0,
            RoutineOutcome::Fail(rc) => rc,
        }
    }
}

/// Common shape for every bench routine.
pub struct Common {
    pub host_id: u8,
    pub motor_id: u8,
}

// ============================================================================
// read
// ============================================================================

pub fn run_read(bus: &CanBus, c: &Common, r: &mut dyn Reporter) -> io::Result<RoutineOutcome> {
    dump_state(bus, c.host_id, c.motor_id, "read", r)?;
    r.report("done", Level::Pass, "read complete");
    Ok(RoutineOutcome::Pass)
}

// ============================================================================
// set_zero (+ optional save)
// ============================================================================

pub fn run_set_zero(
    bus: &CanBus,
    c: &Common,
    save: bool,
    r: &mut dyn Reporter,
) -> io::Result<RoutineOutcome> {
    dump_state(bus, c.host_id, c.motor_id, "initial", r)?;

    let t = Duration::from_millis(500);
    if read_param_f32(bus, c.host_id, c.motor_id, params::VBUS, t)?.is_none() {
        r.report(
            "sanity",
            Level::Fail,
            "no reply from motor on 0x70xx reads.",
        );
        return Ok(RoutineOutcome::Fail(2));
    }
    let mech_vel = read_param_f32(bus, c.host_id, c.motor_id, params::MECH_VEL, t)?;
    if let Some(v) = mech_vel {
        if v.abs() > POWER_SANITY_MAX_VEL_RAD_S {
            r.report(
                "sanity",
                Level::Fail,
                &format!("mechVel = {v} rad/s (shaft spinning)"),
            );
            return Ok(RoutineOutcome::Fail(2));
        }
    }

    r.report("stop", Level::Info, "issuing type-4 stop");
    cmd_stop(bus, c.host_id, c.motor_id, false)?;
    std::thread::sleep(Duration::from_millis(100));

    r.report("zero", Level::Info, "issuing type-6 set_mechanical_zero");
    cmd_set_zero(bus, c.host_id, c.motor_id)?;
    std::thread::sleep(Duration::from_millis(200));

    dump_state(bus, c.host_id, c.motor_id, "after Set Zero (RAM)", r)?;

    if save {
        r.report("save", Level::Info, "issuing type-22 save_params");
        cmd_save_params(bus, c.host_id, c.motor_id)?;
        std::thread::sleep(Duration::from_secs_f32(SAVE_SETTLE_S));
        dump_state(bus, c.host_id, c.motor_id, "after Save to Flash", r)?;
        r.report(
            "save",
            Level::Info,
            "next: power-cycle motor and re-run `read` to confirm persistence",
        );
    }

    r.report("done", Level::Pass, "set_zero complete");
    Ok(RoutineOutcome::Pass)
}

// ============================================================================
// smoke (enable, observe ~1s)
// ============================================================================

pub fn run_smoke(
    bus: &CanBus,
    c: &Common,
    go: bool,
    stop: &AtomicBool,
    r: &mut dyn Reporter,
) -> io::Result<RoutineOutcome> {
    dump_minimal(bus, c.host_id, c.motor_id, "sanity gate", r)?;

    let t = Duration::from_millis(500);
    let vbus = read_param_f32(bus, c.host_id, c.motor_id, params::VBUS, t)?;
    if vbus.is_none() {
        r.report("sanity", Level::Fail, "no reply from motor (VBUS)");
        return Ok(RoutineOutcome::Fail(2));
    }
    if let Some(v) = vbus {
        if v < MIN_VBUS_V {
            r.report(
                "sanity",
                Level::Fail,
                &format!("VBUS {v} V < {MIN_VBUS_V} V"),
            );
            return Ok(RoutineOutcome::Fail(2));
        }
    }
    if let Some(mv) = read_param_f32(bus, c.host_id, c.motor_id, params::MECH_VEL, t)? {
        if mv.abs() > STILL_VEL_GATE_RAD_S {
            r.report(
                "sanity",
                Level::Fail,
                &format!("shaft already moving: mechVel = {mv} rad/s"),
            );
            return Ok(RoutineOutcome::Fail(2));
        }
    }

    if !go {
        r.report(
            "dry_run",
            Level::Info,
            "go flag not set; would enable with spd_ref=0 and observe",
        );
        return Ok(RoutineOutcome::Pass);
    }

    let mut rc = 0i32;
    let mut peak_vel = 0.0f32;
    let mut fb_count = 0u32;

    r.report("setup", Level::Info, "stop + velocity mode + spd_ref=0");
    cmd_stop(bus, c.host_id, c.motor_id, false)?;
    std::thread::sleep(Duration::from_millis(50));
    write_param_u8(bus, c.host_id, c.motor_id, params::RUN_MODE, 2)?;
    std::thread::sleep(Duration::from_millis(20));
    write_param_f32(bus, c.host_id, c.motor_id, params::SPD_REF, 0.0)?;
    std::thread::sleep(Duration::from_millis(20));

    r.report("enable", Level::Info, "type-3 enable");
    cmd_enable(bus, c.host_id, c.motor_id)?;

    let t_end = Instant::now() + Duration::from_secs_f32(OBSERVE_S);
    let mut next_type17 = Instant::now();
    bus.set_read_timeout(Duration::from_secs_f64(f64::from(POLL_RECV_TIMEOUT_S)))?;

    while Instant::now() < t_end {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let now = Instant::now();
        if now >= next_type17 {
            bus.set_read_timeout(Duration::from_millis(200))?;
            if let Some(v) = read_param_f32(
                bus,
                c.host_id,
                c.motor_id,
                params::MECH_VEL,
                Duration::from_millis(200),
            )? {
                peak_vel = peak_vel.max(v.abs());
                if v.abs() > MAX_MECH_VEL_DURING_SMOKE_RAD_S {
                    r.report(
                        "watchdog",
                        Level::Fail,
                        &format!("mechVel |{v}| > {MAX_MECH_VEL_DURING_SMOKE_RAD_S} during enable"),
                    );
                    rc = 3;
                    break;
                }
            }
            next_type17 = now + Duration::from_secs_f32(TYPE17_SAMPLE_PERIOD_S);
        }
        if rc != 0 {
            break;
        }
        match bus.recv() {
            Ok((can_id, data, dlc)) => {
                if comm_type_from_id(can_id) != CommType::MotorFeedback as u8 {
                    continue;
                }
                let raw = strip_eff_flag(can_id);
                let src = ((raw >> 16) & 0xFF) as u8;
                let dst = (raw & 0xFF) as u8;
                if src != c.motor_id || dst != c.host_id {
                    continue;
                }
                if dlc < 8 {
                    continue;
                }
                if let Ok(dec) = decode_motor_feedback(can_id, &data[..dlc]) {
                    fb_count += 1;
                    r.report(
                        "feedback",
                        Level::Info,
                        &format!(
                            "type-2 vel~{:.4} rad/s pos~{:.4} rad T~{:.1} C status=0x{:02X}",
                            dec.vel_rad_s, dec.pos_rad, dec.temp_c, dec.status_byte
                        ),
                    );
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    r.report("defang", Level::Info, "stop + run_mode=0");
    defang_motor(bus, c.host_id, c.motor_id)?;
    bus.set_read_timeout(Duration::from_millis(500))?;
    dump_minimal(bus, c.host_id, c.motor_id, "post-run", r)?;

    if rc == 0 {
        r.report(
            "done",
            Level::Pass,
            &format!(
                "peak |mechVel| from type-17 samples = {peak_vel:.6} rad/s; type-2 frames: {fb_count}"
            ),
        );
        Ok(RoutineOutcome::Pass)
    } else {
        Ok(RoutineOutcome::Fail(rc))
    }
}

// ============================================================================
// jog (trapezoidal velocity ramp)
// ============================================================================

pub fn run_jog(
    bus: &CanBus,
    c: &Common,
    target_vel: f32,
    duration: f32,
    go: bool,
    test_overlimit: bool,
    stop: &AtomicBool,
    r: &mut dyn Reporter,
) -> io::Result<RoutineOutcome> {
    let target = target_vel.clamp(-MAX_TARGET_VEL_RAD_S, MAX_TARGET_VEL_RAD_S);
    if target_vel.abs() > MAX_TARGET_VEL_RAD_S {
        r.report(
            "clamp",
            Level::Warn,
            &format!("clamped |target-vel| to ±{MAX_TARGET_VEL_RAD_S} rad/s (was {target_vel})"),
        );
    }
    if duration.is_nan() || target_vel.is_nan() {
        r.report("sanity", Level::Fail, "NaN duration or target velocity");
        return Ok(RoutineOutcome::Fail(2));
    }
    let duration = duration.clamp(1.0, MAX_DURATION_S);

    dump_minimal(bus, c.host_id, c.motor_id, "pre-check", r)?;
    let pre = sanity_pre_jog(bus, c.host_id, c.motor_id, r)?;
    if pre != 0 {
        return Ok(RoutineOutcome::Fail(pre));
    }

    if !go {
        if test_overlimit {
            r.report(
                "dry_run",
                Level::Info,
                &format!(
                    "would run OVERLIMIT: spd_ref={OVERLIMIT_SPD_REF} for {OVERLIMIT_HOLD_S}s"
                ),
            );
        } else {
            r.report(
                "dry_run",
                Level::Info,
                &format!("would jog at {target} rad/s for {duration} s"),
            );
        }
        return Ok(RoutineOutcome::Pass);
    }

    let rc = if test_overlimit {
        run_overlimit(bus, c.host_id, c.motor_id, stop, r)?
    } else {
        run_jog_ramp(bus, c.host_id, c.motor_id, target, duration, stop, r)?
    };

    r.report("defang", Level::Info, "stop + spd_ref=0 + run_mode=0");
    defang_motor(bus, c.host_id, c.motor_id)?;
    bus.set_read_timeout(Duration::from_millis(500))?;
    dump_minimal(bus, c.host_id, c.motor_id, "post-run", r)?;

    if rc == 0 {
        Ok(RoutineOutcome::Pass)
    } else {
        Ok(RoutineOutcome::Fail(rc))
    }
}

// ============================================================================
// helpers (formerly private to bench_tool.rs)
// ============================================================================

fn dump_state(
    bus: &CanBus,
    host: u8,
    motor: u8,
    label: &str,
    r: &mut dyn Reporter,
) -> io::Result<()> {
    let t = Duration::from_millis(500);
    let run_mode = read_param_u8(bus, host, motor, params::RUN_MODE, t)?;
    let mech_pos = read_param_f32(bus, host, motor, params::MECH_POS, t)?;
    let mech_vel = read_param_f32(bus, host, motor, params::MECH_VEL, t)?;
    let iqf = read_param_f32(bus, host, motor, params::IQF, t)?;
    let vbus = read_param_f32(bus, host, motor, params::VBUS, t)?;
    let limit_spd = read_param_f32(bus, host, motor, params::LIMIT_SPD, t)?;
    let limit_cur = read_param_f32(bus, host, motor, params::LIMIT_CUR, t)?;
    let limit_torque = read_param_f32(bus, host, motor, params::LIMIT_TORQUE, t)?;
    let can_to = session::read_param_u32(bus, host, motor, params::CAN_TIMEOUT, t)?;
    let zero_sta = read_param_u8(bus, host, motor, params::ZERO_STA, t)?;
    let damper = read_param_u8(bus, host, motor, params::DAMPER, t)?;
    let add_off = read_param_f32(bus, host, motor, params::ADD_OFFSET, t)?;

    r.report(label, Level::Info, &format!("run_mode      = {run_mode:?}"));
    r.report(label, Level::Info, &format!("mechPos       = {mech_pos:?}"));
    r.report(label, Level::Info, &format!("mechVel       = {mech_vel:?}"));
    r.report(label, Level::Info, &format!("iqf           = {iqf:?}"));
    r.report(label, Level::Info, &format!("vbus          = {vbus:?}"));
    r.report(
        label,
        Level::Info,
        &format!("limit_spd     = {limit_spd:?}"),
    );
    r.report(
        label,
        Level::Info,
        &format!("limit_cur     = {limit_cur:?}"),
    );
    r.report(
        label,
        Level::Info,
        &format!("limit_torque  = {limit_torque:?}"),
    );
    r.report(label, Level::Info, &format!("can_timeout   = {can_to:?}"));
    r.report(label, Level::Info, &format!("zero_sta      = {zero_sta:?}"));
    r.report(label, Level::Info, &format!("damper        = {damper:?}"));
    r.report(label, Level::Info, &format!("add_offset    = {add_off:?}"));
    Ok(())
}

fn dump_minimal(
    bus: &CanBus,
    host: u8,
    motor: u8,
    label: &str,
    r: &mut dyn Reporter,
) -> io::Result<()> {
    let t = Duration::from_millis(500);
    let vbus = read_param_f32(bus, host, motor, params::VBUS, t)?;
    let mech_vel = read_param_f32(bus, host, motor, params::MECH_VEL, t)?;
    let limit_spd = read_param_f32(bus, host, motor, params::LIMIT_SPD, t)?;
    let run_mode = read_param_u8(bus, host, motor, params::RUN_MODE, t)?;
    r.report(label, Level::Info, &format!("vbus      = {vbus:?} V"));
    r.report(
        label,
        Level::Info,
        &format!("mechVel   = {mech_vel:?} rad/s"),
    );
    r.report(
        label,
        Level::Info,
        &format!("limit_spd = {limit_spd:?} rad/s"),
    );
    r.report(label, Level::Info, &format!("run_mode  = {run_mode:?}"));
    Ok(())
}

fn read_mech_vel_prefer_fb(
    bus: &CanBus,
    host: u8,
    motor: u8,
    last_vel: &mut Option<f32>,
    last_fb_time: &mut Option<Instant>,
) -> io::Result<Option<f32>> {
    bus.set_read_timeout(Duration::from_secs_f64(f64::from(POLL_RECV_TIMEOUT_S)))?;
    if let Some(fb) = drain_motor_feedback(
        bus,
        host,
        motor,
        Duration::from_secs_f64(f64::from(POLL_RECV_TIMEOUT_S)),
    )? {
        *last_vel = Some(fb.vel_rad_s);
        *last_fb_time = Some(Instant::now());
        return Ok(Some(fb.vel_rad_s));
    }
    if let (Some(lv), Some(t)) = (*last_vel, *last_fb_time) {
        if t.elapsed() <= Duration::from_millis(100) {
            return Ok(Some(lv));
        }
    }
    bus.set_read_timeout(Duration::from_millis(250))?;
    read_param_f32(
        bus,
        host,
        motor,
        params::MECH_VEL,
        Duration::from_millis(250),
    )
}

fn sanity_pre_jog(bus: &CanBus, host: u8, motor: u8, r: &mut dyn Reporter) -> io::Result<i32> {
    let t = Duration::from_millis(500);
    let vbus = read_param_f32(bus, host, motor, params::VBUS, t)?;
    if vbus.is_none() {
        r.report("sanity", Level::Fail, "no reply (VBUS)");
        return Ok(2);
    }
    if let Some(v) = vbus {
        if v < MIN_VBUS_V {
            r.report("sanity", Level::Fail, &format!("VBUS {v} V too low"));
            return Ok(2);
        }
    }
    if let Some(mv) = read_param_f32(bus, host, motor, params::MECH_VEL, t)? {
        if mv.abs() > STILL_VEL_GATE_RAD_S {
            r.report(
                "sanity",
                Level::Fail,
                &format!("shaft moving: mechVel = {mv}"),
            );
            return Ok(2);
        }
    }
    let lim = read_param_f32(bus, host, motor, params::LIMIT_SPD, t)?;
    let Some(lim) = lim else {
        r.report("sanity", Level::Fail, "could not read limit_spd");
        return Ok(2);
    };
    if (lim - LIMIT_SPD_EXPECTED).abs() > LIMIT_SPD_TOL {
        r.report(
            "sanity",
            Level::Fail,
            &format!("limit_spd = {lim} rad/s (expected {LIMIT_SPD_EXPECTED} ± {LIMIT_SPD_TOL})"),
        );
        return Ok(2);
    }
    Ok(0)
}

fn run_overlimit(
    bus: &CanBus,
    host: u8,
    motor: u8,
    stop: &AtomicBool,
    r: &mut dyn Reporter,
) -> io::Result<i32> {
    r.report(
        "overlimit",
        Level::Info,
        &format!("spd_ref = {OVERLIMIT_SPD_REF} rad/s for {OVERLIMIT_HOLD_S:.1}s"),
    );
    cmd_stop(bus, host, motor, false)?;
    std::thread::sleep(Duration::from_millis(50));
    write_param_u8(bus, host, motor, params::RUN_MODE, 2)?;
    std::thread::sleep(Duration::from_millis(20));
    write_param_f32(bus, host, motor, params::SPD_REF, 0.0)?;
    std::thread::sleep(Duration::from_millis(20));
    cmd_enable(bus, host, motor)?;
    write_param_f32(bus, host, motor, params::SPD_REF, OVERLIMIT_SPD_REF)?;

    let t_end = Instant::now() + Duration::from_secs_f32(OVERLIMIT_HOLD_S);
    let mut peak = 0.0f32;
    let mut last_v = None;
    let mut last_fb_t = None;

    bus.set_read_timeout(Duration::from_secs_f64(f64::from(POLL_RECV_TIMEOUT_S)))?;
    while Instant::now() < t_end {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let mv = read_mech_vel_prefer_fb(bus, host, motor, &mut last_v, &mut last_fb_t)?;
        if let Some(m) = mv {
            peak = peak.max(m.abs());
            if m.abs() > OVERLIMIT_FAIL_ABOVE {
                r.report(
                    "watchdog",
                    Level::Fail,
                    &format!("|mechVel| = {} > {OVERLIMIT_FAIL_ABOVE}", m.abs()),
                );
                return Ok(4);
            }
        }
        std::thread::sleep(Duration::from_secs_f64(f64::from(TICK_S)));
    }

    r.report(
        "overlimit",
        Level::Info,
        &format!("peak |mechVel| = {peak:.4} rad/s"),
    );
    if !(OVERLIMIT_OK_LO..=OVERLIMIT_OK_HI).contains(&peak) {
        r.report(
            "overlimit",
            Level::Fail,
            &format!("expected peak in [{OVERLIMIT_OK_LO}, {OVERLIMIT_OK_HI}] rad/s"),
        );
        return Ok(5);
    }
    r.report("overlimit", Level::Pass, "overlimit clamp looks active");
    Ok(0)
}

fn desired_spd_at(t_elapsed: f32, target_vel: f32, hold_s: f32) -> f32 {
    if t_elapsed < RAMP_S {
        return target_vel * (t_elapsed / RAMP_S);
    }
    if t_elapsed < RAMP_S + hold_s {
        return target_vel;
    }
    let t2 = t_elapsed - RAMP_S - hold_s;
    if t2 < RAMP_S {
        return target_vel * (1.0 - t2 / RAMP_S);
    }
    0.0
}

fn run_jog_ramp(
    bus: &CanBus,
    host: u8,
    motor: u8,
    target_vel: f32,
    duration_s: f32,
    stop: &AtomicBool,
    r: &mut dyn Reporter,
) -> io::Result<i32> {
    if duration_s < 1.0 {
        r.report("sanity", Level::Fail, "duration must be >= 1.0 s");
        return Ok(2);
    }
    let hold_s = duration_s - 2.0 * RAMP_S;
    if hold_s < 0.0 {
        r.report("sanity", Level::Fail, "internal ramp math error");
        return Ok(2);
    }

    r.report(
        "ramp",
        Level::Info,
        &format!(
            "target_vel={target_vel} rad/s, total={duration_s}s (ramp {RAMP_S}s + hold {hold_s:.2}s + ramp {RAMP_S}s)"
        ),
    );

    cmd_stop(bus, host, motor, false)?;
    std::thread::sleep(Duration::from_millis(50));
    write_param_u8(bus, host, motor, params::RUN_MODE, 2)?;
    std::thread::sleep(Duration::from_millis(20));
    write_param_f32(bus, host, motor, params::SPD_REF, 0.0)?;
    std::thread::sleep(Duration::from_millis(20));
    cmd_enable(bus, host, motor)?;

    let t0 = Instant::now();
    let mut last_v = None;
    let mut last_fb_t = None;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let elapsed = t0.elapsed().as_secs_f32();
        if elapsed >= duration_s {
            break;
        }
        let sp = desired_spd_at(elapsed, target_vel, hold_s);
        write_param_f32(bus, host, motor, params::SPD_REF, sp)?;

        let mv = read_mech_vel_prefer_fb(bus, host, motor, &mut last_v, &mut last_fb_t)?;
        if let Some(m) = mv {
            if m.abs() > WATCHDOG_VEL_RAD_S {
                r.report(
                    "watchdog",
                    Level::Fail,
                    &format!("|mechVel|={} > {WATCHDOG_VEL_RAD_S}", m.abs()),
                );
                return Ok(3);
            }
        }
        std::thread::sleep(Duration::from_secs_f64(f64::from(TICK_S)));
    }

    write_param_f32(bus, host, motor, params::SPD_REF, 0.0)?;
    r.report(
        "ramp",
        Level::Info,
        "ramp complete; holding zero setpoint briefly",
    );
    std::thread::sleep(Duration::from_millis(200));
    r.report("ramp", Level::Pass, "jog ramp finished");
    Ok(0)
}
