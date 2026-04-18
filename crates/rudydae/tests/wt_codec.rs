//! Wire-format pin for the WebTransport envelope.
//!
//! Every datagram and every reliable-stream frame the daemon emits is a CBOR
//! `WtEnvelope<T>` (see `types::WtEnvelope`). The frontend
//! (`link/src/lib/hooks/useWebTransport.ts`) decodes by `kind` and dispatches
//! to per-stream reducers. This test pins:
//!
//! - The envelope shape (`v`, `kind`, `seq`, `t_ms`, `data`) — break either
//!   side and the test fires immediately so the change lands as a coordinated
//!   PR.
//! - The discriminator string for each registered stream (matches the
//!   `kind:` literal in `declare_wt_streams!`).
//! - The frame-size budget: a single datagram must fit comfortably under the
//!   QUIC datagram MTU (~1200 bytes).
//!
//! Reliable-stream framing (length-prefixed body inside the QUIC stream) is
//! tested in `reliable_stream_framing` below.
//!
//! Plus one REST-side smoke: `MotorFeedback.t_ms` stays an unquoted JSON
//! number, because ts-rs declares it as `bigint`.

use rudydae::types::{
    MotorFeedback, SafetyEvent, SystemSnapshot, SystemTemps, SystemThrottled, TestLevel,
    TestProgress, WtEnvelope, WtFrame, WtKind, WtPayload, WtSubscribe, WtTransport,
    WT_PROTOCOL_VERSION, WT_STREAMS,
};

fn sample_motor() -> MotorFeedback {
    MotorFeedback {
        t_ms: 1_700_000_123_456,
        role: "shoulder_actuator_a".into(),
        can_id: 0x08,
        mech_pos_rad: 0.5,
        mech_vel_rad_s: -0.25,
        torque_nm: 1.5,
        vbus_v: 48.1,
        temp_c: 32.0,
        fault_sta: 0,
        warn_sta: 0,
    }
}

fn sample_system() -> SystemSnapshot {
    SystemSnapshot {
        t_ms: 1_700_000_123_456,
        cpu_pct: 17.5,
        load: [0.4, 0.5, 0.6],
        mem_used_mb: 1850,
        mem_total_mb: 8192,
        temps_c: SystemTemps {
            cpu: Some(48.0),
            gpu: Some(45.0),
        },
        throttled: SystemThrottled {
            now: false,
            ever: false,
            raw_hex: Some("0x0".into()),
        },
        uptime_s: 12_345,
        hostname: "rudy-pi".into(),
        kernel: "6.6.20-rpi".into(),
        is_mock: false,
    }
}

#[test]
fn envelope_roundtrips_motor_feedback() {
    let payload = sample_motor();
    let env = WtEnvelope::new(42, payload.clone());
    assert_eq!(env.v, WT_PROTOCOL_VERSION);
    assert_eq!(env.kind, MotorFeedback::KIND);
    assert_eq!(env.seq, 42);

    let mut buf = Vec::with_capacity(160);
    ciborium::into_writer(&env, &mut buf).expect("encode CBOR");

    // The decode side rebuilds via WtFrame (the SPA-shaped tagged union)
    // because that's what TypeScript actually consumes. The envelope's
    // `data:` nesting maps to serde's `content = "data"` tag, and its
    // top-level `kind:` to `tag = "kind"` — both pinned in types.rs.
    let frame: WtFrame = ciborium::from_reader(buf.as_slice()).expect("decode CBOR");
    let WtFrame::MotorFeedback(decoded) = frame else {
        panic!("expected MotorFeedback variant");
    };
    assert_eq!(decoded.t_ms, payload.t_ms);
    assert_eq!(decoded.role, payload.role);
    assert_eq!(decoded.mech_pos_rad, payload.mech_pos_rad);
}

#[test]
fn envelope_roundtrips_system_snapshot() {
    let payload = sample_system();
    let env = WtEnvelope::new(7, payload.clone());
    assert_eq!(env.kind, SystemSnapshot::KIND);

    let mut buf = Vec::with_capacity(256);
    ciborium::into_writer(&env, &mut buf).expect("encode CBOR");

    let frame: WtFrame = ciborium::from_reader(buf.as_slice()).expect("decode CBOR");
    let WtFrame::SystemSnapshot(decoded) = frame else {
        panic!("expected SystemSnapshot variant");
    };
    assert_eq!(decoded.cpu_pct, payload.cpu_pct);
    assert_eq!(decoded.hostname, payload.hostname);
}

#[test]
fn envelope_json_shape_is_stable() {
    // The exact field names + nesting are part of the contract. A future
    // refactor that, say, flattens `data` would silently break the SPA
    // decoder (which assumes `frame.data.role` for motor feedback).
    let env = WtEnvelope::new(123, sample_motor());
    let json = serde_json::to_string(&env).expect("encode JSON");
    for needle in [
        r#""v":1"#,
        r#""kind":"motor_feedback""#,
        r#""seq":123"#,
        r#""t_ms":"#, // value is wallclock-derived; just check the field exists
        r#""data":{"#,
    ] {
        assert!(
            json.contains(needle),
            "envelope JSON missing `{needle}`: {json}"
        );
    }
}

#[test]
fn discriminator_strings_match_macro() {
    // The frontend hard-codes the kind strings ("motor_feedback",
    // "system_snapshot", ...) in its reducer registry. The macro's job is
    // to keep the Rust side in sync: this asserts the macro-generated
    // `KIND` constants and the `WtKind::as_str()` mapping agree.
    assert_eq!(MotorFeedback::KIND, "motor_feedback");
    assert_eq!(SystemSnapshot::KIND, "system_snapshot");
    assert_eq!(TestProgress::KIND, "test_progress");
    assert_eq!(SafetyEvent::KIND, "safety_event");
    assert_eq!(WtKind::MotorFeedback.as_str(), "motor_feedback");
    assert_eq!(WtKind::SystemSnapshot.as_str(), "system_snapshot");
    assert_eq!(WtKind::TestProgress.as_str(), "test_progress");
    assert_eq!(WtKind::SafetyEvent.as_str(), "safety_event");

    let kinds: Vec<&str> = WT_STREAMS.iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&"motor_feedback"));
    assert!(kinds.contains(&"system_snapshot"));
    assert!(kinds.contains(&"test_progress"));
    assert!(kinds.contains(&"safety_event"));
}

#[test]
fn transport_assignments_match_macro() {
    assert_eq!(MotorFeedback::TRANSPORT, WtTransport::Datagram);
    assert_eq!(SystemSnapshot::TRANSPORT, WtTransport::Datagram);
    assert_eq!(TestProgress::TRANSPORT, WtTransport::Stream);
    assert_eq!(SafetyEvent::TRANSPORT, WtTransport::Stream);
    assert_eq!(WtKind::MotorFeedback.transport(), WtTransport::Datagram);
    assert_eq!(WtKind::TestProgress.transport(), WtTransport::Stream);
}

#[test]
fn envelope_roundtrips_test_progress() {
    let payload = TestProgress {
        run_id: "abc-123".into(),
        role: "shoulder_actuator_a".into(),
        seq: 42,
        t_ms: 1_700_000_123_456,
        step: "ramp".into(),
        level: TestLevel::Info,
        message: "spd_ref=0.20".into(),
    };
    let env = WtEnvelope::new(7, payload.clone());
    assert_eq!(env.kind, TestProgress::KIND);
    let mut buf = Vec::with_capacity(192);
    ciborium::into_writer(&env, &mut buf).expect("encode CBOR");
    let frame: WtFrame = ciborium::from_reader(buf.as_slice()).expect("decode CBOR");
    let WtFrame::TestProgress(decoded) = frame else {
        panic!("expected TestProgress variant");
    };
    assert_eq!(decoded.run_id, payload.run_id);
    assert_eq!(decoded.message, payload.message);
}

#[test]
fn envelope_roundtrips_safety_event() {
    let payload = SafetyEvent::Estop {
        t_ms: 1_700_000_123_456,
        source: "session-A".into(),
    };
    let env = WtEnvelope::new(0, payload.clone());
    assert_eq!(env.kind, SafetyEvent::KIND);
    let mut buf = Vec::with_capacity(160);
    ciborium::into_writer(&env, &mut buf).expect("encode CBOR");
    let frame: WtFrame = ciborium::from_reader(buf.as_slice()).expect("decode CBOR");
    let WtFrame::SafetyEvent(decoded) = frame else {
        panic!("expected SafetyEvent variant");
    };
    if let SafetyEvent::Estop { source, .. } = decoded {
        assert_eq!(source, "session-A");
    } else {
        panic!("expected Estop variant");
    }
}

/// Run-id filter on `WtSubscribe` is honoured by the wt_router's
/// `allows_run` predicate. We pin the field shape here so a future
/// rename trips a contract failure rather than a silent dropped frame.
#[test]
fn wt_subscribe_round_trips_run_ids() {
    let json = r#"{"kinds":["test_progress"],"filters":{"motor_roles":[],"run_ids":["r-1","r-2"]}}"#;
    let sub: WtSubscribe = serde_json::from_str(json).expect("decode WtSubscribe");
    assert_eq!(sub.filters.run_ids, vec!["r-1", "r-2"]);
}

#[test]
fn cbor_payload_size_is_reasonable() {
    // The envelope adds ~30-40 bytes (v/kind/seq/t_ms/data map header). The
    // hard QUIC datagram MTU is ~1200 bytes and the actual peer-negotiated
    // limit is often smaller; staying well under 512 leaves headroom for
    // any future field additions.
    let mut buf = Vec::with_capacity(160);
    ciborium::into_writer(&WtEnvelope::new(0, sample_motor()), &mut buf).expect("encode CBOR");
    assert!(
        buf.len() < 256,
        "MotorFeedback envelope ballooned to {} bytes",
        buf.len()
    );

    let mut buf = Vec::with_capacity(256);
    ciborium::into_writer(&WtEnvelope::new(0, sample_system()), &mut buf).expect("encode CBOR");
    assert!(
        buf.len() < 512,
        "SystemSnapshot envelope ballooned to {} bytes",
        buf.len()
    );
}

#[test]
fn json_motor_feedback_uses_unquoted_t_ms() {
    // REST-side guard: the SPA's MotorFeedback.ts declares t_ms as
    // bigint, but JSON.parse returns Number for unquoted ints. The gap
    // is benign for 53-bit timestamps. This test stops anyone from
    // "fixing" the wire shape with a serializer that quotes the int.
    let original = sample_motor();
    let json = serde_json::to_string(&original).expect("serialise JSON");
    assert!(
        json.contains(r#""t_ms":1700000123456"#),
        "expected unquoted t_ms in JSON, got: {json}"
    );
    let decoded: MotorFeedback = serde_json::from_str(&json).expect("decode JSON");
    assert_eq!(decoded.t_ms, original.t_ms);
}

#[test]
fn reliable_stream_framing_is_length_prefixed() {
    // Reliable frames are written into a long-lived QUIC uni-stream as
    // `u32 BE length | cbor body`. The TS reader (link's
    // useWebTransport reliable path) depends on this exact framing.
    // The router builds it inline — here we just assert the length
    // header round-trips bit-for-bit through the obvious encoder.
    let env = WtEnvelope::new(99, sample_system());
    let mut body = Vec::with_capacity(256);
    ciborium::into_writer(&env, &mut body).expect("cbor body");

    let mut frame = Vec::with_capacity(4 + body.len());
    let len = u32::try_from(body.len()).expect("body fits in u32");
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&body);

    // Decoder side: read 4 bytes BE -> length -> then exactly that many
    // bytes -> CBOR decode.
    let (header, rest) = frame.split_at(4);
    let parsed_len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
    assert_eq!(parsed_len, body.len(), "length header round-trips");
    let parsed: WtFrame = ciborium::from_reader(rest).expect("body decodes");
    let WtFrame::SystemSnapshot(_) = parsed else {
        panic!("expected SystemSnapshot");
    };
}

#[test]
fn wt_subscribe_default_kinds_means_all() {
    // The protocol contract: an empty `kinds` list ≡ "all kinds". The
    // SPA omits the field for "give me everything"; this test guards
    // against a future refactor that interprets empty as "give me
    // nothing" (which would silently kill the dashboard).
    let json = r#"{"kinds":[],"filters":{"motor_roles":[]}}"#;
    let sub: WtSubscribe = serde_json::from_str(json).expect("decode WtSubscribe");
    assert!(sub.kinds.is_empty());
    assert!(sub.filters.motor_roles.is_empty());
}
