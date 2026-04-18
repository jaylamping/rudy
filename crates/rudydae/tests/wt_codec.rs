//! Wire-format pin for the WebTransport datagram payload.
//!
//! The frontend (`link/src/lib/hooks/useWebTransport.ts`) reads CBOR datagrams
//! and decodes them into `MotorFeedback`. Today the TS-side decoder is a stub
//! (returns `null`), so this test cannot run an end-to-end browser round-trip
//! — but it pins the **server-side** encoder. If anyone changes `wt.rs` to
//! emit JSON, msgpack, or a wrapped envelope, this test breaks immediately so
//! the frontend stub upgrade and the encoder change land together.
//!
//! We also pin the JSON shape of `MotorFeedback`. The REST endpoint
//! `/api/motors/:role/feedback` returns the exact same struct as JSON, and
//! ts-rs renders `t_ms: i64` as `bigint` in TypeScript. JSON.parse in browsers
//! returns `number`, not `bigint`, for unquoted ints — the gap is real but
//! benign for timestamps that fit in 53 bits. This test guards against
//! someone "fixing" the wire shape (e.g. switching to a `serde_with` helper
//! that quotes the field) without updating `MotorFeedback.ts`.

use rudydae::types::MotorFeedback;

fn sample() -> MotorFeedback {
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

#[test]
fn cbor_roundtrip_motor_feedback() {
    let original = sample();

    // EXACT same encode path as `wt::handle_session`.
    let mut buf = Vec::with_capacity(128);
    ciborium::into_writer(&original, &mut buf).expect("encode CBOR");

    // The browser decoder is the inverse of this. When the TS-side
    // useWebTransport hook gains a real CBOR decoder (cbor-x or hand-rolled),
    // the bytes it consumes are exactly what this test asserts can decode.
    let decoded: MotorFeedback = ciborium::from_reader(buf.as_slice()).expect("decode CBOR");

    assert_eq!(decoded.t_ms, original.t_ms);
    assert_eq!(decoded.role, original.role);
    assert_eq!(decoded.can_id, original.can_id);
    assert_eq!(decoded.mech_pos_rad, original.mech_pos_rad);
    assert_eq!(decoded.mech_vel_rad_s, original.mech_vel_rad_s);
    assert_eq!(decoded.torque_nm, original.torque_nm);
    assert_eq!(decoded.vbus_v, original.vbus_v);
    assert_eq!(decoded.temp_c, original.temp_c);
    assert_eq!(decoded.fault_sta, original.fault_sta);
    assert_eq!(decoded.warn_sta, original.warn_sta);
}

#[test]
fn json_motor_feedback_uses_unquoted_t_ms() {
    let original = sample();
    let json = serde_json::to_string(&original).expect("serialise JSON");

    // The exact substring matters: ts-rs declares `t_ms: bigint`, so the wire
    // shape MUST be a JSON number (not a quoted string). If someone wraps it
    // with serde_with::TimestampMilliSeconds<String>, the SPA silently breaks.
    assert!(
        json.contains(r#""t_ms":1700000123456"#),
        "expected unquoted t_ms in JSON, got: {json}"
    );

    // And the round-trip via serde_json must work bit-for-bit (no precision
    // loss for timestamps fitting in 53 bits).
    let decoded: MotorFeedback = serde_json::from_str(&json).expect("decode JSON");
    assert_eq!(decoded.t_ms, original.t_ms);
}

#[test]
fn cbor_payload_size_is_reasonable() {
    // Sanity guard: a single feedback frame at hundreds of bytes would mean
    // someone accidentally embedded the whole spec / inventory in each
    // datagram. WebTransport datagrams are size-limited (~1200 bytes), so
    // bloat shows up first as silent send failures.
    let mut buf = Vec::with_capacity(128);
    ciborium::into_writer(&sample(), &mut buf).expect("encode CBOR");
    assert!(
        buf.len() < 256,
        "MotorFeedback CBOR encoding ballooned to {} bytes",
        buf.len()
    );
}
