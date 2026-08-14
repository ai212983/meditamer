#[path = "../../../src/firmware/display/panel/refresh_tracking.rs"]
mod refresh_tracking;

use refresh_tracking::{CompletedRefresh, RefreshTracking};

#[test]
fn partial_count_tracks_only_completed_partial_refreshes() {
    let mut tracking = RefreshTracking::new();

    tracking.record_success(CompletedRefresh::NoChange);
    assert_eq!(tracking.partial_count(), 0);

    tracking.record_success(CompletedRefresh::Partial);
    assert_eq!(tracking.partial_count(), 1);

    tracking.record_success(CompletedRefresh::Full);
    assert_eq!(tracking.partial_count(), 0);
}

#[test]
fn failed_transaction_forces_full_recovery_until_full_succeeds() {
    let mut tracking = RefreshTracking::new();
    tracking.record_success(CompletedRefresh::Partial);

    tracking.record_failure();
    assert!(tracking.recovery_required());
    assert!(tracking.should_request_full(None));
    assert_eq!(tracking.partial_count(), 0);

    tracking.record_success(CompletedRefresh::Full);
    assert!(!tracking.recovery_required());
    assert!(!tracking.should_request_full(None));
}

#[test]
fn automatic_full_refresh_limit_is_enforced_after_successful_partials() {
    let mut tracking = RefreshTracking::new();
    tracking.record_success(CompletedRefresh::Partial);
    tracking.record_success(CompletedRefresh::Partial);

    assert!(!tracking.should_request_full(Some(3)));
    tracking.record_success(CompletedRefresh::Partial);
    assert!(tracking.should_request_full(Some(3)));
}
