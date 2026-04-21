use super::*;
use crate::types::{WtKind, WtSubscribe, WtSubscribeFilters};

/// CBOR roundtrip pinning. The wire format here is what the SPA's
/// `clientStream.ts` writes; if serde / ciborium ever change the
/// internally-tagged-enum-of-newtype shape this test fires before
/// the dead-man jog silently breaks.
fn roundtrip(frame: ClientFrame) -> ClientFrame {
    let mut buf = Vec::new();
    ciborium::into_writer(&frame, &mut buf).expect("encode");
    ciborium::de::from_reader(buf.as_slice()).expect("decode")
}

#[test]
fn subscribe_round_trips() {
    let f = ClientFrame::Subscribe(WtSubscribe {
        kinds: vec![WtKind::MotorFeedback, WtKind::MotionStatus],
        filters: WtSubscribeFilters {
            motor_roles: vec!["shoulder_a".into()],
            run_ids: vec![],
        },
    });
    let back = roundtrip(f);
    match back {
        ClientFrame::Subscribe(sub) => {
            assert_eq!(sub.kinds.len(), 2);
            assert_eq!(sub.filters.motor_roles, vec!["shoulder_a".to_string()]);
        }
        _ => panic!("variant mismatch after roundtrip"),
    }
}

#[test]
fn motion_jog_round_trips() {
    let back = roundtrip(ClientFrame::MotionJog {
        role: "shoulder_a".into(),
        vel_rad_s: 0.25,
    });
    match back {
        ClientFrame::MotionJog { role, vel_rad_s } => {
            assert_eq!(role, "shoulder_a");
            assert!((vel_rad_s - 0.25).abs() < 1e-6);
        }
        _ => panic!("variant mismatch"),
    }
}

#[test]
fn motion_heartbeat_round_trips() {
    let back = roundtrip(ClientFrame::MotionHeartbeat {
        role: "shoulder_a".into(),
    });
    assert!(matches!(back, ClientFrame::MotionHeartbeat { role } if role == "shoulder_a"));
}

#[test]
fn motion_stop_round_trips() {
    let back = roundtrip(ClientFrame::MotionStop {
        role: "shoulder_a".into(),
    });
    assert!(matches!(back, ClientFrame::MotionStop { role } if role == "shoulder_a"));
}

/// The SPA emits `{kind: "subscribe", kinds: [...], filters: {...}}`
/// (i.e. the tag is *flattened* into the inner struct). Pin that
/// shape so a future serde adjacent-tag flip doesn't silently land.
#[test]
fn subscribe_uses_internally_tagged_shape() {
    let f = ClientFrame::Subscribe(WtSubscribe {
        kinds: vec![],
        filters: WtSubscribeFilters::default(),
    });
    let mut buf = Vec::new();
    ciborium::into_writer(&f, &mut buf).expect("encode");
    // Re-decode as a generic CBOR value and assert the top-level
    // map has both `kind` and `kinds` keys.
    let val: ciborium::Value = ciborium::de::from_reader(buf.as_slice()).expect("decode");
    let map = val.as_map().expect("map");
    let mut saw_kind = false;
    let mut saw_kinds = false;
    for (k, _v) in map {
        if let Some(s) = k.as_text() {
            if s == "kind" {
                saw_kind = true;
            }
            if s == "kinds" {
                saw_kinds = true;
            }
        }
    }
    assert!(saw_kind, "missing `kind` discriminator");
    assert!(saw_kinds, "missing flattened `kinds` field");
}
