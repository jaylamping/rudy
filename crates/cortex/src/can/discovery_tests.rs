use super::*;

struct MockReader {
    /// Sequence of replies to return on successive calls. Each entry is
    /// `Ok(Some(bytes))` (success), `Ok(None)` (read-fail status reply),
    /// or `Err(_)` (timeout). Calls past the end yield a fresh timeout.
    replies: std::sync::Mutex<Vec<io::Result<Option<[u8; 4]>>>>,
}

impl MockReader {
    fn always(reply: io::Result<Option<[u8; 4]>>) -> Self {
        // 16 entries is enough for any test since `RobstrideProbe`
        // touches at most max_attempts * 2 registers (= 4 by default).
        let mut v = Vec::with_capacity(16);
        for _ in 0..16 {
            v.push(clone_result(&reply));
        }
        Self {
            replies: std::sync::Mutex::new(v),
        }
    }

    fn sequence(replies: Vec<io::Result<Option<[u8; 4]>>>) -> Self {
        Self {
            replies: std::sync::Mutex::new(replies),
        }
    }
}

fn clone_result(r: &io::Result<Option<[u8; 4]>>) -> io::Result<Option<[u8; 4]>> {
    match r {
        Ok(v) => Ok(*v),
        Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
    }
}

impl BusParamReader for MockReader {
    fn read_type17_register(
        &self,
        _iface: &str,
        _motor_id: u8,
        _index: u16,
        _timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>> {
        let mut q = self.replies.lock().unwrap();
        if q.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "mock timeout"));
        }
        q.remove(0)
    }
}

struct MockBroadcaster {
    replies: std::collections::BTreeMap<String, Vec<RawBroadcastReply>>,
}

impl BusBroadcaster for MockBroadcaster {
    fn broadcast_get_device_id(
        &self,
        iface: &str,
        _total_listen: Duration,
    ) -> io::Result<Vec<RawBroadcastReply>> {
        Ok(self.replies.get(iface).cloned().unwrap_or_default())
    }
}

#[test]
fn robstride_probe_none_reply_counts_as_present() {
    let reader = MockReader::always(Ok(None));
    let p = RobstrideProbe::default();
    let dev = p
        .probe(&reader, "can0", 0x55, Duration::from_millis(1))
        .unwrap();
    assert_eq!(dev.bus, "can0");
    assert_eq!(dev.can_id, 0x55);
    assert_eq!(dev.family_hint, "robstride");
    assert!(dev.identification_payload.is_some());
}

#[test]
fn robstride_probe_timeout_yields_none() {
    let reader = MockReader::always(Err(io::Error::new(io::ErrorKind::TimedOut, "")));
    let p = RobstrideProbe::default();
    assert!(p
        .probe(&reader, "can0", 0x55, Duration::from_millis(1))
        .is_none());
}

#[test]
fn robstride_probe_falls_back_to_mcu_id_after_firmware_timeout() {
    // First two reads (firmware_version × 2 attempts) time out; the third
    // (mcu_id, attempt 1) succeeds. With max_attempts=2 that's exactly
    // the 3rd entry the reader hands out.
    let reader = MockReader::sequence(vec![
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
        Ok(Some([0xDE, 0xAD, 0xBE, 0xEF])),
    ]);
    let p = RobstrideProbe::default();
    let dev = p
        .probe(&reader, "can0", 0x10, Duration::from_millis(1))
        .expect("device should be detected via mcu_id fallback");
    let payload = dev.identification_payload.expect("payload");
    assert_eq!(payload["param_name"], "mcu_id");
    assert_eq!(payload["param_index"], "0x7005");
}

#[test]
fn robstride_probe_first_attempt_retries_before_giving_up() {
    // With max_attempts=2: firmware tries twice (both timeout), then
    // mcu_id tries twice (both timeout). 4 total reader calls before
    // we declare absent.
    let reader = MockReader::sequence(vec![
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
        Err(io::Error::new(io::ErrorKind::TimedOut, "")),
    ]);
    let p = RobstrideProbe::default();
    assert!(p
        .probe(&reader, "can0", 0x10, Duration::from_millis(1))
        .is_none());
    assert!(reader.replies.lock().unwrap().is_empty());
}

#[test]
fn registry_first_probe_wins() {
    let reader = MockReader::always(Ok(Some(*b"v1\0\0")));
    let reg = DeviceProbeRegistry::with_default_probes();
    let (dev, att) = reg.probe_one_id(&reader, "can0", 0x10, Duration::from_millis(1));
    assert!(dev.is_some());
    assert_eq!(att.len(), 1);
    assert!(att[0].found);
    assert_eq!(att[0].attempt, 1);
}

#[test]
fn registry_runs_broadcast_and_dedups_responders() {
    let reg = DeviceProbeRegistry::with_default_probes();
    let mut by_iface = std::collections::BTreeMap::new();
    by_iface.insert(
        "can0".to_string(),
        vec![
            RawBroadcastReply {
                motor_id: 0x10,
                comm_type: 0x00,
                data: [0; 8],
            },
            RawBroadcastReply {
                motor_id: 0x10, // duplicate, should collapse
                comm_type: 0x02,
                data: [1; 8],
            },
            RawBroadcastReply {
                motor_id: 0x12,
                comm_type: 0x02,
                data: [2; 8],
            },
        ],
    );
    let bcast = MockBroadcaster { replies: by_iface };

    let responders = reg.run_broadcasts(&bcast, "can0", Duration::from_millis(50));
    assert_eq!(responders.len(), 2);
    let ids: Vec<u8> = responders.iter().map(|r| r.motor_id).collect();
    assert_eq!(ids, vec![0x10, 0x12]);
    assert!(responders.iter().all(|r| r.family_hint == "robstride"));
}

#[test]
fn registry_run_broadcasts_returns_empty_when_no_replies() {
    let reg = DeviceProbeRegistry::with_default_probes();
    let bcast = MockBroadcaster {
        replies: std::collections::BTreeMap::new(),
    };
    let responders = reg.run_broadcasts(&bcast, "can0", Duration::from_millis(50));
    assert!(responders.is_empty());
}
