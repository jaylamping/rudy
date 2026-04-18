//! RS03 commissioning CLI (SocketCAN). Canonical per ADR-0003.
//!
//! Replaces `tools/robstride/bench_*.py` for Pi-side bring-up. Wraps the
//! shared `driver::rs03::tests` library so the routines stay 1:1 with what
//! `rudydae` runs from the operator console.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use driver::rs03::tests::{self, Common, Level, Reporter, RoutineOutcome};
use driver::socketcan_bus::CanBus;
use tracing::warn;
use tracing_subscriber::EnvFilter;

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
    Read(CommonArgs),
    /// Set mechanical zero (+ optional flash save).
    SetZero {
        #[command(flatten)]
        common: CommonArgs,
        #[arg(long)]
        save: bool,
    },
    /// Enable smoke test (velocity mode, spd_ref=0).
    Smoke {
        #[command(flatten)]
        common: CommonArgs,
        #[arg(long, help = "Actually send CAN motion / enable frames")]
        go: bool,
    },
    /// Velocity jog with trapezoidal spd_ref ramp.
    Jog {
        #[command(flatten)]
        common: CommonArgs,
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
struct CommonArgs {
    #[arg(long, default_value = "can0")]
    iface: String,
    #[arg(long, default_value = "0x08", value_parser = parse_u8_hex)]
    motor_id: u8,
    #[arg(long, default_value = "0xFD", value_parser = parse_u8_hex)]
    host_id: u8,
    #[arg(short, long)]
    verbose: bool,
}

impl CommonArgs {
    fn to_lib(&self) -> Common {
        Common {
            host_id: self.host_id,
            motor_id: self.motor_id,
        }
    }
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

/// Reporter that prints to stdout/stderr with the same format as the old
/// inlined bench_tool. Lets the CLI continue to feel identical to the
/// original after the library refactor.
struct PrintReporter;

impl Reporter for PrintReporter {
    fn report(&mut self, step: &str, level: Level, message: &str) {
        match level {
            Level::Info => println!("[{step}] {message}"),
            Level::Warn => println!("[{step}] WARN: {message}"),
            Level::Pass => println!("[{step}] PASS: {message}"),
            Level::Fail => eprintln!("[{step}] FAIL: {message}"),
        }
    }
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

    let mut reporter = PrintReporter;

    let mut run = || -> io::Result<RoutineOutcome> {
        match &cli.command {
            Commands::Read(c) => {
                let bus = open_bus(&c.iface)?;
                println!(
                    "bound {}, motor 0x{:02X}, host 0x{:02X}",
                    c.iface, c.motor_id, c.host_id
                );
                tests::run_read(&bus, &c.to_lib(), &mut reporter)
            }
            Commands::SetZero { common, save } => {
                let bus = open_bus(&common.iface)?;
                println!(
                    "bound {}, motor 0x{:02X}, host 0x{:02X}",
                    common.iface, common.motor_id, common.host_id
                );
                tests::run_set_zero(&bus, &common.to_lib(), *save, &mut reporter)
            }
            Commands::Smoke { common, go } => {
                let bus = open_bus(&common.iface)?;
                println!(
                    "bound {}, motor 0x{:02X}, host 0x{:02X}",
                    common.iface, common.motor_id, common.host_id
                );
                tests::run_smoke(&bus, &common.to_lib(), *go, &stop_flag, &mut reporter)
            }
            Commands::Jog {
                common,
                target_vel,
                duration,
                go,
                test_overlimit,
            } => {
                let bus = open_bus(&common.iface)?;
                println!(
                    "bound {}, motor 0x{:02X}, host 0x{:02X}",
                    common.iface, common.motor_id, common.host_id
                );
                tests::run_jog(
                    &bus,
                    &common.to_lib(),
                    &tests::JogParams {
                        target_vel: *target_vel,
                        duration: *duration,
                        go: *go,
                        test_overlimit: *test_overlimit,
                    },
                    &stop_flag,
                    &mut reporter,
                )
            }
        }
    };

    match run() {
        Ok(outcome) => std::process::exit(outcome.exit_code()),
        Err(e) => {
            eprintln!("bench_tool: {e}");
            std::process::exit(1);
        }
    }
}
