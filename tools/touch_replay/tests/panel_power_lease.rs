#![allow(dead_code)]

#[path = "../../../src/firmware/runtime/display_task/lvgl/panel_power_lease.rs"]
mod panel_power_lease;

use panel_power_lease::{
    LeaseMaintenance, LeaseRefreshKind, PanelPowerLease, PanelPowerLeasePolicy,
};

#[test]
fn configured_policy_leases_every_partial_for_three_seconds() {
    let policy = PanelPowerLeasePolicy::configured();
    assert!(policy.enabled());
    assert_eq!(policy.idle_ms(), 3_000);
    assert!(policy.should_leave_on_for_partial());
}

#[test]
fn successful_partial_starts_three_second_idle_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert_eq!(lease.take_maintenance(3_999), LeaseMaintenance::None);
    assert_eq!(
        lease.take_maintenance(4_000),
        LeaseMaintenance::ShutDown { active_ms: 3_000 }
    );
}

#[test]
fn each_successful_partial_renews_the_idle_deadline() {
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
fn explicit_panel_shutdown_cancels_the_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    lease.mark_panel_off();
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn disabled_policy_never_leases_a_partial() {
    let policy = PanelPowerLeasePolicy::new(0);
    let mut lease = PanelPowerLease::new(policy);

    assert!(!policy.should_leave_on_for_partial());
    assert!(!lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn no_change_does_not_renew_an_existing_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert!(!lease.record_refresh_success(LeaseRefreshKind::NoChange, 3_000));
    assert_eq!(
        lease.take_maintenance(4_000),
        LeaseMaintenance::ShutDown { active_ms: 3_000 }
    );
}

#[test]
fn full_refresh_cancels_an_existing_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_refresh_success(LeaseRefreshKind::Partial, 1_000));
    assert!(!lease.record_refresh_success(LeaseRefreshKind::Full, 2_000));
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}
