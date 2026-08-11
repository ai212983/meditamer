#[path = "../../../src/firmware/runtime/display_task/lvgl/panel_power_lease.rs"]
mod panel_power_lease;

use panel_power_lease::{
    LeaseMaintenance, PanelPowerLease, PanelPowerLeasePolicy,
};

#[test]
fn normal_build_keeps_experimental_lease_disabled() {
    let policy = PanelPowerLeasePolicy::configured();

    assert!(!policy.enabled());
    assert_eq!(policy.idle_ms(), 0);
}

#[test]
fn successful_partial_arms_and_expires_lease() {
    let policy = PanelPowerLeasePolicy::new(3_000);
    let mut lease = PanelPowerLease::new(policy);

    assert_eq!(lease.policy(), policy);
    assert!(lease.record_partial_success(1_000));
    assert_eq!(lease.take_maintenance(3_999), LeaseMaintenance::None);
    assert_eq!(
        lease.take_maintenance(4_000),
        LeaseMaintenance::ShutDown { active_ms: 3_000 }
    );
}

#[test]
fn repeated_partial_renews_deadline_without_resetting_active_time() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_partial_success(1_000));
    assert!(lease.record_partial_success(3_000));
    assert_eq!(lease.take_maintenance(5_999), LeaseMaintenance::None);
    assert_eq!(
        lease.take_maintenance(6_000),
        LeaseMaintenance::ShutDown { active_ms: 5_000 }
    );
}

#[test]
fn explicit_panel_off_cancels_lease() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(3_000));

    assert!(lease.record_partial_success(1_000));
    lease.mark_panel_off();

    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}

#[test]
fn disabled_policy_never_arms() {
    let mut lease = PanelPowerLease::new(PanelPowerLeasePolicy::new(0));

    assert!(!lease.record_partial_success(1_000));
    assert_eq!(lease.take_maintenance(10_000), LeaseMaintenance::None);
}
