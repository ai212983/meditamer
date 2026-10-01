#[path = "../../../products/meditamer/src/firmware/display/panel/refresh_tracking.rs"]
mod refresh_tracking;

use refresh_tracking::{CompletedRefresh, RefreshTracking};

#[test]
fn partial_refreshes_do_not_accumulate_clean_refresh_debt() {
    let mut tracking = RefreshTracking::new();
    tracking.record_success(CompletedRefresh::Full);

    for _ in 0..100 {
        tracking.record_success(CompletedRefresh::Partial);
    }

    assert!(!tracking.should_request_full());
}

#[test]
fn failed_transaction_forces_clean_recovery_until_full_succeeds() {
    let mut tracking = RefreshTracking::new();
    tracking.record_success(CompletedRefresh::Full);

    assert_eq!(tracking.next_deadline_ms(), None);

    tracking.record_failure(100);
    assert!(tracking.recovery_required());
    assert!(tracking.should_request_full());
    assert_eq!(tracking.next_deadline_ms(), Some(1_100));

    tracking.record_success(CompletedRefresh::Full);
    assert!(!tracking.recovery_required());
    assert!(!tracking.should_request_full());
    assert_eq!(tracking.next_deadline_ms(), None);
}

#[test]
fn rejected_startup_retries_without_dirty_input_and_recovers_on_a_full_scan() {
    let mut tracking = RefreshTracking::new();
    assert!(!tracking.startup_complete());
    assert!(!tracking.recovery_retry_due(0));
    assert!(tracking.should_request_full());

    tracking.record_failure(250);
    tracking.record_success(CompletedRefresh::NoChange);
    assert!(!tracking.recovery_retry_due(1_249));
    assert!(tracking.recovery_retry_due(1_250));
    assert!(!tracking.startup_complete());

    tracking.record_success(CompletedRefresh::Full);
    assert!(tracking.startup_complete());
    assert!(!tracking.recovery_retry_due(10_000));
    assert!(!tracking.recovery_required());
}

#[test]
fn persistent_failure_waits_a_full_interval_after_each_attempt() {
    let mut tracking = RefreshTracking::new();
    for completed_ms in [250, 3_000, 7_500] {
        tracking.record_failure(completed_ms);
        assert!(!tracking.recovery_retry_due(completed_ms));
        assert!(!tracking.recovery_retry_due(completed_ms + 999));
        assert!(tracking.recovery_retry_due(completed_ms + 1_000));
    }
}

#[test]
fn post_startup_failures_use_the_same_retry_cadence() {
    let mut tracking = RefreshTracking::new();
    tracking.record_success(CompletedRefresh::Full);
    tracking.record_failure(5_000);

    assert!(tracking.startup_complete());
    assert!(tracking.recovery_required());
    assert!(!tracking.recovery_retry_due(5_999));
    assert!(tracking.recovery_retry_due(6_000));
}

#[test]
fn only_full_scans_complete_startup() {
    for completed in [CompletedRefresh::Partial, CompletedRefresh::NoChange] {
        let mut tracking = RefreshTracking::new();
        tracking.record_success(completed);
        assert!(!tracking.startup_complete());
        assert!(tracking.should_request_full());
    }
}
