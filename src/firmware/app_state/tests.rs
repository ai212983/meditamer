use crate::firmware::config::APP_STATE_STORE_RECORD_LEN;

use super::{
    actions::AppStateApplyStatus,
    engine::AppStateEngine,
    events::AppStateCommand,
    store::PersistedAppState,
    types::{BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode, ServiceFlags},
    AppStateSnapshot,
};

fn checksum8(bytes: &[u8]) -> u8 {
    let mut acc = 0x5Au8;
    for &byte in bytes {
        acc ^= byte.rotate_left(1);
    }
    acc
}

#[test]
fn boot_transition_initializing_to_operating_day_defaults() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let result = engine.apply(AppStateCommand::BootComplete);
    assert!(result.changed());
    assert!(matches!(result.after.phase, super::types::Phase::Operating));
    assert!(matches!(result.after.base, BaseMode::Day));
    assert!(matches!(
        result.after.day_background,
        DayBackground::Shanshui
    ));
    assert!(matches!(result.after.overlay, OverlayMode::None));
}

#[test]
fn day_background_switching_via_state_command() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let _ = engine.apply(AppStateCommand::BootComplete);
    let result = engine.apply(AppStateCommand::SetDayBackground(
        DayBackground::Suminagashi,
    ));
    assert!(result.changed());
    assert!(matches!(
        result.after.day_background,
        DayBackground::Suminagashi
    ));
}

#[test]
fn overlay_clock_allowed_on_day_mode() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let _ = engine.apply(AppStateCommand::BootComplete);
    let result = engine.apply(AppStateCommand::SetOverlay(OverlayMode::Clock));
    assert!(matches!(result.status, AppStateApplyStatus::Applied));
    assert!(matches!(result.after.overlay, OverlayMode::Clock));
}

#[test]
fn overlay_clock_rejected_and_autocleared_on_touch_wizard() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let _ = engine.apply(AppStateCommand::BootComplete);
    let _ = engine.apply(AppStateCommand::SetOverlay(OverlayMode::Clock));
    let to_wizard = engine.apply(AppStateCommand::SetBase(BaseMode::TouchWizard));
    assert!(matches!(to_wizard.after.overlay, OverlayMode::None));

    let invalid = engine.apply(AppStateCommand::SetOverlay(OverlayMode::Clock));
    assert!(matches!(
        invalid.status,
        AppStateApplyStatus::InvalidTransition
    ));
    assert!(matches!(invalid.after.overlay, OverlayMode::None));
}

#[test]
fn service_flag_toggles_report_changes() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let _ = engine.apply(AppStateCommand::BootComplete);

    let upload_on = engine.apply(AppStateCommand::SetUpload(true));
    assert!(upload_on.changed());
    assert!(upload_on.services_changed());
    assert!(upload_on.persist_required());

    let upload_on_again = engine.apply(AppStateCommand::SetUpload(true));
    assert!(!upload_on_again.changed());
    assert!(!upload_on_again.services_changed());

    let assets_off = engine.apply(AppStateCommand::SetAssets(false));
    assert!(assets_off.changed());
    assert!(assets_off.services_changed());
    assert!(assets_off.persist_required());
}

#[test]
fn persisted_v1_roundtrip() {
    let persisted = PersistedAppState {
        base: BaseMode::Day,
        day_background: DayBackground::Suminagashi,
        overlay: OverlayMode::Clock,
        services: ServiceFlags {
            upload_enabled: true,
            asset_reads_enabled: false,
        },
        diag_kind: DiagKind::Debug,
        diag_targets: DiagTargets::from_persisted((1 << 0) | (1 << 1) | (1 << 4)),
    };

    let record = persisted.record_bytes();
    let decoded = PersistedAppState::from_record(&record).expect("decode v1 record");
    assert_eq!(decoded, persisted);
}

#[test]
fn persisted_non_v1_rejected() {
    let mut record = PersistedAppState::default().record_bytes();
    record[4] = 7;
    record[APP_STATE_STORE_RECORD_LEN - 1] = checksum8(&record[..APP_STATE_STORE_RECORD_LEN - 1]);
    assert!(PersistedAppState::from_record(&record).is_none());
}

#[test]
fn snapshot_from_persisted_sanitizes_touch_wizard_overlay() {
    let persisted = PersistedAppState {
        base: BaseMode::TouchWizard,
        day_background: DayBackground::Shanshui,
        overlay: OverlayMode::Clock,
        services: ServiceFlags::normal(),
        diag_kind: DiagKind::None,
        diag_targets: DiagTargets::none(),
    };

    let snapshot = AppStateSnapshot::from_persisted_sanitized(persisted);
    assert!(matches!(snapshot.overlay, OverlayMode::None));
}

#[test]
fn snapshot_from_persisted_preserves_day_overlay() {
    let persisted = PersistedAppState {
        base: BaseMode::Day,
        day_background: DayBackground::Suminagashi,
        overlay: OverlayMode::Clock,
        services: ServiceFlags::normal(),
        diag_kind: DiagKind::Debug,
        diag_targets: DiagTargets::from_persisted(1),
    };

    let snapshot = AppStateSnapshot::from_persisted_sanitized(persisted);
    assert!(matches!(snapshot.overlay, OverlayMode::Clock));
    assert!(matches!(snapshot.base, BaseMode::Day));
    assert!(matches!(
        snapshot.day_background,
        DayBackground::Suminagashi
    ));
}

#[test]
fn invalid_or_empty_store_falls_back_to_defaults() {
    let empty = [0xFFu8; APP_STATE_STORE_RECORD_LEN];
    let fallback_from_empty = PersistedAppState::from_record(&empty).unwrap_or_default();
    assert_eq!(fallback_from_empty, PersistedAppState::default());

    let invalid = [0u8; APP_STATE_STORE_RECORD_LEN];
    let fallback_from_invalid = PersistedAppState::from_record(&invalid).unwrap_or_default();
    assert_eq!(fallback_from_invalid, PersistedAppState::default());
}
