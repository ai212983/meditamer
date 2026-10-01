#[path = "../../../products/meditamer/src/firmware/ui/lvgl/screen_update_state.rs"]
mod screen_update_state;

// Architectural regression guard: the embedded display owner cannot be linked
// into this host harness. Its admission boundary must remain independent of
// network/service state; panel state is tested behaviorally by the modules above
// and panel_refresh_tracking.
#[test]
fn upload_availability_cannot_gate_interaction_or_recovery() {
    let paths = [
        "display.rs",
        "display/app_events.rs",
        "display/app_events/repaint.rs",
        "display/app_events/panel_fixture.rs",
        "display/presentation.rs",
        "imu/tasks/acquisition.rs",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../products/meditamer/src/firmware");
    for path in paths {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        for forbidden in [
            "upload_enabled",
            "upload_transfers_enabled",
            "active_http_connections",
            "sd_upload_session_active",
            "UploadBlocked",
            "ImuSuppressionReason::Upload",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} couples interaction to {forbidden}; service availability and idle keepalive must not block UI/recovery"
            );
        }
    }
}

#[test]
fn service_transition_does_not_override_frontlight() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../products/meditamer/src/firmware/display/state.rs"),
    )
    .unwrap();
    assert!(!source.contains("frontlight_off"));
    assert!(!source.contains("set_brightness"));
}
