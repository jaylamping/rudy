use super::*;

struct MockReader {
    ok_reply: Option<Option<[u8; 4]>>,
}

impl BusParamReader for MockReader {
    fn read_type17_register(
        &self,
        _iface: &str,
        _motor_id: u8,
        _index: u16,
        _timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>> {
        self.ok_reply
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "mock timeout"))
    }
}

#[test]
fn robstride_probe_none_reply_counts_as_present() {
    let reader = MockReader {
        ok_reply: Some(None),
    };
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
    let reader = MockReader { ok_reply: None };
    let p = RobstrideProbe::default();
    assert!(p
        .probe(&reader, "can0", 0x55, Duration::from_millis(1))
        .is_none());
}

#[test]
fn registry_first_probe_wins() {
    let reader = MockReader {
        ok_reply: Some(Some(*b"v1\0\0")),
    };
    let reg = DeviceProbeRegistry::with_default_probes();
    let (dev, att) = reg.probe_one_id(&reader, "can0", 0x10, Duration::from_millis(1));
    assert!(dev.is_some());
    assert_eq!(att.len(), 1);
    assert!(att[0].found);
}
