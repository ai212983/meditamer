use crate::firmware::config::APP_STATE_STORE_RECORD_LEN;

use super::{
    engine::AppStateEngine,
    events::AppStateCommand,
    store::PersistedAppState,
    types::{DiagKind, DiagTargets, ServiceFlags},
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
fn boot_transition_initializing_to_operating_defaults() {
    let mut engine = AppStateEngine::new(AppStateSnapshot::default());
    let result = engine.apply(AppStateCommand::BootComplete);
    assert!(result.changed());
    assert!(matches!(result.after.phase, super::types::Phase::Operating));
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
}

#[test]
fn persisted_roundtrip() {
    let persisted = PersistedAppState {
        services: ServiceFlags {
            upload_enabled: true,
        },
        diag_kind: DiagKind::Debug,
        diag_targets: DiagTargets::from_persisted((1 << 0) | (1 << 1) | (1 << 4)),
    };

    let record = persisted.record_bytes();
    let decoded = PersistedAppState::from_record(&record).expect("decode v3 record");
    assert_eq!(decoded, persisted);
}

#[test]
fn persisted_non_v3_rejected() {
    let mut record = PersistedAppState::default().record_bytes();
    record[4] = 7;
    record[APP_STATE_STORE_RECORD_LEN - 1] = checksum8(&record[..APP_STATE_STORE_RECORD_LEN - 1]);
    assert!(PersistedAppState::from_record(&record).is_none());
}

#[test]
fn snapshot_from_persisted_preserves_remaining_state() {
    let persisted = PersistedAppState {
        services: ServiceFlags::normal(),
        diag_kind: DiagKind::Debug,
        diag_targets: DiagTargets::from_persisted(1),
    };

    let snapshot = AppStateSnapshot::from_persisted_sanitized(persisted);
    assert_eq!(snapshot.services, persisted.services);
    assert_eq!(snapshot.diag_kind, persisted.diag_kind);
    assert_eq!(snapshot.diag_targets, persisted.diag_targets);
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
