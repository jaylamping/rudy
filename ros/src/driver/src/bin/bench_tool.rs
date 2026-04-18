//! RS03 commissioning CLI (SocketCAN). Canonical per ADR-0003.
//!
//! Replaces `tools/robstride/bench_*.py` for Pi-side bring-up.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Args, Parser, Subcommand};
use driver::rs03::params;
use driver::rs03::session::{
    self, cmd_enable, cmd_save_params, cmd_set_zero as motor_set_zero, cmd_stop, defang_motor,
    drain_motor_feedback, read_param_f32, read_param_u8, write_param_f32, write_param_u8,
};
use driver::rs03::{comm_type_from_id, decode_motor_feedback, strip_eff_flag, CommType};
use driver::socketcan_bus::CanBus;
use tracing::{debug, warn};
use tracing_subscriber::EnvFilter;

// --- Same hard caps as Python bench scripts --------------------------------

const MAX_TARGET_VEL_RAD_S: f32 = 0.5;
const MAX_DURATION_S: f32 = 3.0;
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

// ----------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "bench_tool",
    version,
    about = "RS03 bench / commissioning CLI (Rust)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Type-17 read of key 0x70xx observables (like bench_set_zero --read-only).
    Read(Common),
    /// Set mechanical zero (+ optional flash save).
    SetZero {
        #[command(flatten)]
        common: Common,
        #[arg(long)]
        save: bool,
    },
    /// Enable smoke test (velocity mode, spd_ref=0).
    Smoke {
        #[command(flatten)]
        common: Common,
        #[arg(long, help = "Actually send CAN motion / enable frames")]
        go: bool,
    },
    /// Velocity jog with trapezoidal spd_ref ramp.
    Jog {
        #[command(flatten)]
        common: Common,
        #[arg(long, default_value_t = 0.2)]
        target_vel: f32,
        #[arg(long, default_value_t = 2.0)]
        duration: f32,
        #[arg(long)]
        go: bool,
        #[arg(long)]
        test_overlimit: bool,
    },
}

#[derive(Args, Debug, Clone)]
struct Common {
    #[arg(long, default_value = "can0")]
    iface: String,
    #[arg(long, default_value = "0x08", value_parser = parse_u8_hex)]
    motor_id: u8,
    #[arg(long, default_value = "0xFD", value_parser = parse_u8_hex)]
    host_id: u8,
    #[arg(short, long)]
    verbose: bool,
}

fn parse_u8_hex(s: &str) -> Result<u8, String> {
    let s = s.trim();
    if let Some(r) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(r, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<u8>().map_err(|e| e.to_string())
    }
}

fn init_tracing(verbose: bool) {
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn open_bus(iface: &str) -> io::Result<CanBus> {
    let bus = CanBus::open(iface)?;
    bus.set_read_timeout(Duration::from_millis(500))?;
    Ok(bus)
}

fn dump_state(bus: &CanBus, host: u8, motor: u8, label: &str) -> io::Result<()> {
    let t = Duration::from_millis(500);
    println!("\n--- state: {label} ---");
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

    println!("  run_mode (0x7005) = {run_mode:?}");
    println!("  mechPos      (0x7019)     = {mech_pos:?}");
    println!("  mechVel      (0x701B)     = {mech_vel:?}");
    println!("  iqf          (0x701A)     = {iqf:?}");
    println!("  VBUS         (0x701C)     = {vbus:?}");
    println!("  limit_spd    (0x7017)     = {limit_spd:?}");
    println!("  limit_cur    (0x7018)     = {limit_cur:?}");
    println!("  limit_torque (0x700B)     = {limit_torque:?}");
    println!("  canTimeout   (0x7028)     = {can_to:?}");
    println!("  zero_sta     (0x7029)     = {zero_sta:?}");
    println!("  damper       (0x702A)     = {damper:?}");
    println!("  add_offset   (0x702B)     = {add_off:?}");
    Ok(())
}

fn cmd_read(common: Common) -> io::Result<i32> {
    let bus = open_bus(&common.iface)?;
    println!(
        "bound {}, motor 0x{:02X}, host 0x{:02X}",
        common.iface, common.motor_id, common.host_id
    );
    dump_state(&bus, common.host_id, common.motor_id, "read")?;
    Ok(0)
}

fn run_set_zero(common: Common, save: bool) -> io::Result<i32> {
    let bus = open_bus(&common.iface)?;
    println!(
        "bound {}, motor 0x{:02X}, host 0x{:02X}",
        common.iface, common.motor_id, common.host_id
    );
    dump_state(&bus, common.host_id, common.motor_id, "initial")?;

    let t = Duration::from_millis(500);
    if read_param_f32(&bus, common.host_id, common.motor_id, params::VBUS, t)?.is_none() {
        eprintln!("ABORT: no reply from motor on 0x70xx reads.");
        return Ok(2);
    }
    let mech_vel = read_param_f32(&bus, common.host_id, common.motor_id, params::MECH_VEL, t)?;
    if let Some(v) = mech_vel {
        if v.abs() > POWER_SANITY_MAX_VEL_RAD_S {
            eprintln!("ABORT: mechVel = {v} rad/s (shaft spinning).");
            return Ok(2);
        }
    }

    println!("\n>>> Stopping motor (type 4)");
    cmd_stop(&bus, common.host_id, common.motor_id, false)?;
    std::thread::sleep(Duration::from_millis(100));

    println!(">>> Set Mechanical Zero (type 6, byte0=1)");
    motor_set_zero(&bus, common.host_id, common.motor_id)?;
    std::thread::sleep(Duration::from_millis(200));

    dump_state(
        &bus,
        common.host_id,
        common.motor_id,
        "after Set Zero (RAM)",
    )?;

    if save {
        println!("\n>>> Save Parameters (type 22)");
        cmd_save_params(&bus, common.host_id, common.motor_id)?;
        std::thread::sleep(Duration::from_secs_f32(SAVE_SETTLE_S));
        dump_state(&bus, common.host_id, common.motor_id, "after Save to Flash")?;
        println!("\nNext: power-cycle motor and re-run `read` to confirm persistence.");
    }

    Ok(0)
}

fn dump_minimal(bus: &CanBus, host: u8, motor: u8, label: &str) -> io::Result<()> {
    let t = Duration::from_millis(500);
    println!("\n--- {label} ---");
    let vbus = read_param_f32(bus, host, motor, params::VBUS, t)?;
    let mech_vel = read_param_f32(bus, host, motor, params::MECH_VEL, t)?;
    let limit_spd = read_param_f32(bus, host, motor, params::LIMIT_SPD, t)?;
    let run_mode = read_param_u8(bus, host, motor, params::RUN_MODE, t)?;
    println!("  VBUS = {vbus:?} V");
    println!("  mechVel   = {mech_vel:?} rad/s");
    println!("  limit_spd = {limit_spd:?} rad/s");
    println!("  run_mode  = {run_mode:?}");
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

fn sanity_pre_jog(bus: &CanBus, host: u8, motor: u8) -> io::Result<i32> {
    let t = Duration::from_millis(500);
    let vbus = read_param_f32(bus, host, motor, params::VBUS, t)?;
    if vbus.is_none() {
        eprintln!("FAIL: no reply (VBUS).");
        return Ok(2);
    }
    if let Some(v) = vbus {
        if v < MIN_VBUS_V {
            eprintln!("FAIL: VBUS {v} V too low.");
            return Ok(2);
        }
    }
    if let Some(mv) = read_param_f32(bus, host, motor, params::MECH_VEL, t)? {
        if mv.abs() > STILL_VEL_GATE_RAD_S {
            eprintln!("FAIL: shaft moving: mechVel = {mv}");
            return Ok(2);
        }
    }
    let lim = read_param_f32(bus, host, motor, params::LIMIT_SPD, t)?;
    let Some(lim) = lim else {
        eprintln!("FAIL: could not read limit_spd.");
        return Ok(2);
    };
    if (lim - LIMIT_SPD_EXPECTED).abs() > LIMIT_SPD_TOL {
        eprintln!(
            "FAIL: limit_spd = {lim} rad/s (expected {LIMIT_SPD_EXPECTED} ± {LIMIT_SPD_TOL})."
        );
        return Ok(2);
    }
    Ok(0)
}

fn run_overlimit(
    bus: &CanBus,
    host: u8,
    motor: u8,
    verbose: bool,
    stop: &AtomicBool,
) -> io::Result<i32> {
    println!(
        "\n>>> OVERLIMIT TEST: spd_ref = {OVERLIMIT_SPD_REF} rad/s for {OVERLIMIT_HOLD_S:.1}s"
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
                eprintln!("FAIL: |mechVel| = {} > {OVERLIMIT_FAIL_ABOVE}", m.abs());
                return Ok(4);
            }
        }
        if verbose {
            debug!(?mv, peak, "overlimit sample");
        }
        std::thread::sleep(Duration::from_secs_f64(f64::from(TICK_S)));
    }

    println!("  peak |mechVel| = {peak:.4} rad/s");
    if !(OVERLIMIT_OK_LO..=OVERLIMIT_OK_HI).contains(&peak) {
        eprintln!("FAIL: expected peak in [{OVERLIMIT_OK_LO}, {OVERLIMIT_OK_HI}] rad/s.");
        return Ok(5);
    }
    println!("PASS: overlimit clamp looks active.");
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
    verbose: bool,
    stop: &AtomicBool,
) -> io::Result<i32> {
    if duration_s < 1.0 {
        eprintln!("FAIL: duration must be >= 1.0 s.");
        return Ok(2);
    }
    let hold_s = duration_s - 2.0 * RAMP_S;
    if hold_s < 0.0 {
        eprintln!("FAIL: internal ramp math error.");
        return Ok(2);
    }

    println!(
        "\n>>> JOG: target_vel={target_vel} rad/s, total={duration_s}s (ramp {RAMP_S}s + hold {hold_s:.2}s + ramp {RAMP_S}s)"
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
                eprintln!(
                    "\nFAIL: watchdog: |mechVel|={} > {WATCHDOG_VEL_RAD_S}",
                    m.abs()
                );
                return Ok(3);
            }
        }
        if verbose {
            debug!(elapsed, sp, ?mv, "jog tick");
        }
        std::thread::sleep(Duration::from_secs_f64(f64::from(TICK_S)));
    }

    write_param_f32(bus, host, motor, params::SPD_REF, 0.0)?;
    println!(">>> Ramp complete; holding zero setpoint briefly...");
    std::thread::sleep(Duration::from_millis(200));
    println!("PASS: jog ramp finished.");
    Ok(0)
}

fn cmd_jog(
    common: Common,
    target_vel: f32,
    duration: f32,
    go: bool,
    test_overlimit: bool,
    stop: &AtomicBool,
) -> io::Result<i32> {
    let target = target_vel.clamp(-MAX_TARGET_VEL_RAD_S, MAX_TARGET_VEL_RAD_S);
    if target_vel.abs() > MAX_TARGET_VEL_RAD_S {
        println!("NOTE: clamped |target-vel| to ±{MAX_TARGET_VEL_RAD_S} rad/s (was {target_vel})");
    }

    if duration.is_nan() || target_vel.is_nan() {
        eprintln!("FAIL: NaN duration or target velocity.");
        return Ok(2);
    }
    let duration = duration.clamp(1.0, MAX_DURATION_S);

    let bus = open_bus(&common.iface)?;
    println!(
        "bound {}, motor 0x{:02X}, host 0x{:02X}",
        common.iface, common.motor_id, common.host_id
    );
    dump_minimal(&bus, common.host_id, common.motor_id, "pre-check")?;

    let pre = sanity_pre_jog(&bus, common.host_id, common.motor_id)?;
    if pre != 0 {
        return Ok(pre);
    }

    if !go {
        println!("\nDry-run (--go not set). Would:");
        if test_overlimit {
            println!("  Run OVERLIMIT: spd_ref={OVERLIMIT_SPD_REF} for {OVERLIMIT_HOLD_S}s");
        } else {
            println!("  Velocity jog: target {target} rad/s, duration {duration} s");
        }
        return Ok(0);
    }

    let rc = if test_overlimit {
        run_overlimit(&bus, common.host_id, common.motor_id, common.verbose, stop)?
    } else {
        run_jog_ramp(
            &bus,
            common.host_id,
            common.motor_id,
            target,
            duration,
            common.verbose,
            stop,
        )?
    };

    println!("\n>>> Defang: stop + spd_ref=0 + run_mode=0");
    defang_motor(&bus, common.host_id, common.motor_id)?;
    bus.set_read_timeout(Duration::from_millis(500))?;
    dump_minimal(&bus, common.host_id, common.motor_id, "post-run")?;

    Ok(rc)
}

fn cmd_smoke(common: Common, go: bool, stop: &AtomicBool) -> io::Result<i32> {
    let bus = open_bus(&common.iface)?;
    println!(
        "bound {}, motor 0x{:02X}, host 0x{:02X}",
        common.iface, common.motor_id, common.host_id
    );
    dump_minimal(&bus, common.host_id, common.motor_id, "sanity gate")?;

    let t = Duration::from_millis(500);
    let vbus = read_param_f32(&bus, common.host_id, common.motor_id, params::VBUS, t)?;
    if vbus.is_none() {
        eprintln!("FAIL: no reply from motor (VBUS).");
        return Ok(2);
    }
    if let Some(v) = vbus {
        if v < MIN_VBUS_V {
            eprintln!("FAIL: VBUS {v} V < {MIN_VBUS_V} V.");
            return Ok(2);
        }
    }
    if let Some(mv) = read_param_f32(&bus, common.host_id, common.motor_id, params::MECH_VEL, t)? {
        if mv.abs() > STILL_VEL_GATE_RAD_S {
            eprintln!("FAIL: shaft already moving: mechVel = {mv} rad/s.");
            return Ok(2);
        }
    }

    if !go {
        println!("\nDry-run only (--go not set). Would enable with spd_ref=0 and observe.");
        return Ok(0);
    }

    let mut rc = 0i32;
    let mut peak_vel = 0.0f32;
    let mut fb_count = 0u32;

    println!("\n>>> Pre: stop + velocity mode + spd_ref=0");
    cmd_stop(&bus, common.host_id, common.motor_id, false)?;
    std::thread::sleep(Duration::from_millis(50));
    write_param_u8(&bus, common.host_id, common.motor_id, params::RUN_MODE, 2)?;
    std::thread::sleep(Duration::from_millis(20));
    write_param_f32(&bus, common.host_id, common.motor_id, params::SPD_REF, 0.0)?;
    std::thread::sleep(Duration::from_millis(20));

    println!(">>> Enable (type 3)");
    cmd_enable(&bus, common.host_id, common.motor_id)?;

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
                &bus,
                common.host_id,
                common.motor_id,
                params::MECH_VEL,
                Duration::from_millis(200),
            )? {
                peak_vel = peak_vel.max(v.abs());
                if v.abs() > MAX_MECH_VEL_DURING_SMOKE_RAD_S {
                    eprintln!(
                        "FAIL: mechVel |{v}| > {MAX_MECH_VEL_DURING_SMOKE_RAD_S} during enable."
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
                if src != common.motor_id || dst != common.host_id {
                    continue;
                }
                if dlc < 8 {
                    continue;
                }
                match decode_motor_feedback(can_id, &data[..dlc]) {
                    Ok(dec) => {
                        fb_count += 1;
                        println!(
                            "  [FB#{fb_count}] type-2 vel~{:.4} rad/s pos~{:.4} rad T~{:.1} C status=0x{:02X}",
                            dec.vel_rad_s,
                            dec.pos_rad,
                            dec.temp_c,
                            dec.status_byte
                        );
                    }
                    Err(_) => continue,
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

    if rc == 0 {
        println!(
            "\nPASS: peak |mechVel| from type-17 samples = {peak_vel:.6} rad/s; type-2 frames: {fb_count}"
        );
    }

    println!("\n>>> Defang: stop + run_mode=0");
    defang_motor(&bus, common.host_id, common.motor_id)?;
    bus.set_read_timeout(Duration::from_millis(500))?;
    dump_minimal(&bus, common.host_id, common.motor_id, "post-run")?;

    Ok(rc)
}

fn main() {
    let cli = Cli::parse();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let s = stop_flag.clone();
    let _ = ctrlc::set_handler(move || {
        s.store(true, Ordering::SeqCst);
        warn!("Ctrl-C: requesting safe stop");
    });

    let verbose = match &cli.command {
        Commands::Read(c) => c.verbose,
        Commands::SetZero { common, .. } => common.verbose,
        Commands::Smoke { common, .. } => common.verbose,
        Commands::Jog { common, .. } => common.verbose,
    };
    init_tracing(verbose);

    let run = || -> io::Result<i32> {
        match &cli.command {
            Commands::Read(c) => cmd_read(c.clone()),
            Commands::SetZero { common, save } => run_set_zero(common.clone(), *save),
            Commands::Smoke { common, go } => cmd_smoke(common.clone(), *go, &stop_flag),
            Commands::Jog {
                common,
                target_vel,
                duration,
                go,
                test_overlimit,
            } => cmd_jog(
                common.clone(),
                *target_vel,
                *duration,
                *go,
                *test_overlimit,
                &stop_flag,
            ),
        }
    };

    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("bench_tool: {e}");
            std::process::exit(1);
        }
    }
}
