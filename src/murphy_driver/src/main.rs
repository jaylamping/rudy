//! Murphy CAN driver node (binary).
//!
//! ROS 2 lifecycle integration will land here once `rclrs` is wired into the workspace.
//! Today this binary provides a small CLI for bring-up and CI smoke checks.

use std::env;

use murphy_driver::{MitCommand, RobstrideCodec};

fn print_usage() {
    eprintln!(
        "murphy_driver_node — Murphy Robstride CAN tools\n\
         \n\
         Usage:\n\
           murphy_driver_node print-mit --motor-id <u8>\n\
         \n\
         Example:\n\
           murphy_driver_node print-mit --motor-id 1\n"
    );
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    match args[1].as_str() {
        "print-mit" => {
            let mut motor_id: u8 = 1;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--motor-id" && i + 1 < args.len() {
                    motor_id = args[i + 1].parse().expect("motor id");
                    i += 2;
                } else {
                    i += 1;
                }
            }
            let codec = RobstrideCodec;
            let cmd = MitCommand {
                position_rad: 0.0,
                velocity_rad_s: 0.0,
                kp: 0.0,
                kd: 0.0,
                torque_ff_nm: 0.0,
            };
            let (id, data) = codec.encode_mit(motor_id, cmd).expect("encode");
            println!("id=0x{:08X} data={:02X?}", id, data);
        }
        _ => {
            print_usage();
            std::process::exit(2);
        }
    }
}
