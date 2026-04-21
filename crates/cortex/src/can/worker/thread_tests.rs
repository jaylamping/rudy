use std::collections::HashMap;
use std::io;
use std::sync::mpsc;

use super::command::{Cmd, PendingKey, WriteValue};
use super::pin::auto_assign_cpu;

#[test]
fn auto_assign_cpu_skips_core_zero() {
    assert_eq!(auto_assign_cpu(0, 4), Some(1));
    assert_eq!(auto_assign_cpu(1, 4), Some(2));
    assert_eq!(auto_assign_cpu(2, 4), Some(3));
    assert_eq!(auto_assign_cpu(3, 4), Some(1));
}

#[test]
fn auto_assign_cpu_single_core_returns_none() {
    assert_eq!(auto_assign_cpu(0, 1), None);
    assert_eq!(auto_assign_cpu(2, 1), None);
}

#[test]
fn auto_assign_cpu_two_cores_uses_only_core_one() {
    assert_eq!(auto_assign_cpu(0, 2), Some(1));
    assert_eq!(auto_assign_cpu(1, 2), Some(1));
    assert_eq!(auto_assign_cpu(7, 2), Some(1));
}

#[test]
fn write_value_variants_carry_typed_payloads() {
    let cases = [
        WriteValue::F32(1.5),
        WriteValue::U8(42),
        WriteValue::U32(0xDEAD_BEEF),
    ];
    for c in cases {
        let copy = c;
        match (c, copy) {
            (WriteValue::F32(a), WriteValue::F32(b)) => assert!((a - b).abs() < 1e-9),
            (WriteValue::U8(a), WriteValue::U8(b)) => assert_eq!(a, b),
            (WriteValue::U32(a), WriteValue::U32(b)) => assert_eq!(a, b),
            _ => panic!("variant mismatch after Copy"),
        }
    }
}

#[test]
fn bus_handle_submit_after_drop_reports_broken_pipe() {
    let (tx, rx) = mpsc::channel::<Cmd>();
    drop(rx);
    let (reply_tx, _reply_rx) = mpsc::channel::<io::Result<()>>();
    let cmd = Cmd::Enable {
        motor_id: 0x08,
        host_id: 0xFD,
        reply: reply_tx,
    };
    let send_err = tx.send(cmd).unwrap_err();
    let io_err: io::Error = io::Error::new(io::ErrorKind::BrokenPipe, format!("{send_err}"));
    assert_eq!(io_err.kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn pending_key_distinguishes_motor_and_index() {
    let a = PendingKey {
        motor_id: 0x08,
        index: 0x7019,
    };
    let b = PendingKey {
        motor_id: 0x09,
        index: 0x7019,
    };
    let c = PendingKey {
        motor_id: 0x08,
        index: 0x701A,
    };
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
    let mut map = HashMap::new();
    map.insert(a, "a");
    map.insert(b, "b");
    map.insert(c, "c");
    assert_eq!(map.len(), 3);
}
