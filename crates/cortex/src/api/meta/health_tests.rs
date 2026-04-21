use super::build_health_response;

#[test]
fn status_tracks_spa_embed_presence() {
    let ok = build_health_response(true, false, true);
    assert!(ok.healthy);
    assert_eq!(ok.status, "ok");

    let degraded = build_health_response(false, false, true);
    assert!(!degraded.healthy);
    assert_eq!(degraded.status, "degraded");
}
