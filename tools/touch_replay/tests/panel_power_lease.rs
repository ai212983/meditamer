#![allow(dead_code)]

#[path = "../../../src/firmware/display/panel/lease.rs"]
mod panel_power_lease;

use panel_power_lease::{
    should_shutdown_parked_panel, LeaseMaintenance, LeaseRefreshKind, PanelPowerLease,
    PanelPowerLeasePolicy,
};

#[test]
fn configured_policy_holds_every_partial_for_three_seconds() {
    let policy = PanelPowerLeasePolicy::configured();

    assert!(policy.enabled());
    assert_eq!(policy.idle_ms(), 3_000);
    assert!(policy.should_hold_terminal_state_for_partial());
}

#[test]
fn successful_partial_arms_and_expires_lease() {
    let policy = PanelPowerLeasePolicy::new(3_000);
    let mut lease = PanelPowerLease::new(policy);

    assert_eq!(lease.policy(), policy);
    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert_eq!(lease.take_maintenance(3_999), LeaseMaintenance::None);
    assert_eq!(
        lease.take_maintenance(4_000),
        LeaseMaintenance::ShutDown { active_ms: 3_000 }
    );
}

#[test]
fn repeated_partial_renews_deadline_without_resetting_active_time() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 3_000));
    assert_eq!(lease.take_maintenance(5_999), LeaseMaintenance::None);
    assert_eq!(
        lease.take_maintenance(6_000),
        LeaseMaintenance::ShutDown { active_ms: 5_000 }
    );
}

#[test]
fn explicit_panel_off_cancels_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    lease.mark_panel_off();

    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn disabled_policy_never_arms() {
    let policy = PanelPowerLeasePolicy::new(0);
    let mut lease = PanelPowerLease::new(policy);

    assert!(!policy.should_hold_terminal_state_for_partial());
    assert!(!lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn no_change_does_not_renew_existing_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert!(!lease.record_refresh_success(LeaseRefreshKind::NoChange, 3_000));
    assert_eq!(
        lease.take_maintenance(4_000),
        LeaseMaintenance::ShutDown { active_ms: 3_000 }
    );
}

#[test]
fn full_refresh_cancels_existing_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert!(!lease.record_refresh_success(LeaseRefreshKind::Full, 2_000));
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn held_full_fallback_requires_shutdown_before_clients_resume() {
    assert!(should_shutdown_parked_panel(
        true,
        LeaseRefreshKind::Full,
        true
    ));

    assert!(!should_shutdown_parked_panel(
        false,
        LeaseRefreshKind::Full,
        true
    ));
    assert!(!should_shutdown_parked_panel(
        true,
        LeaseRefreshKind::Full,
        false
    ));
    assert!(!should_shutdown_parked_panel(
        true,
        LeaseRefreshKind::NoChange,
        false
    ));
    assert!(!should_shutdown_parked_panel(
        true,
        LeaseRefreshKind::Partial,
        false
    ));
}
