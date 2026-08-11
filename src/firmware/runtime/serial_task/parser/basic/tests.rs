use super::*;

use crate::firmware::app_state::DiagKind;
use crate::firmware::runtime::serial_task::commands::StateSetOperation;
use crate::firmware::runtime::{
    scheduling::SchedulerProfile, serial_task::commands::SchedulerOperation,
};

#[test]
fn parses_scheduler_profile_control() {
    assert!(matches!(
        parse_scheduler_command(b"SCHEDPROFILE"),
        Some(SchedulerOperation::Status)
    ));
    assert!(matches!(
        parse_scheduler_command(b"SCHEDPROFILE AUTO"),
        Some(SchedulerOperation::Automatic)
    ));
    assert!(matches!(
        parse_scheduler_command(b"SCHEDPROFILE UPLOAD"),
        Some(SchedulerOperation::Profile(SchedulerProfile::Upload))
    ));
    assert!(matches!(
        parse_scheduler_command(b"SCHEDPROFILE DIAGNOSTICS"),
        Some(SchedulerOperation::Profile(SchedulerProfile::Diagnostics))
    ));
    assert!(parse_scheduler_command(b"SCHEDPROFILE FAST").is_none());
}

#[test]
fn parses_only_the_exact_ui_cycle_step_command() {
    assert!(parse_ui_cycle_step_command(b"  UISTEP\r\n"));
    assert!(!parse_ui_cycle_step_command(b"UISTEP 3"));
    assert!(!parse_ui_cycle_step_command(b"UI CYCLE"));
}

#[cfg(feature = "ble-foundation")]
#[test]
fn parses_bounded_ble_probe_commands() {
    assert!(parse_ble_probe_start_command(b" BLEPROBE START\r\n"));
    assert!(parse_ble_probe_status_command(b"BLEPROBE"));
    assert!(parse_ble_probe_status_command(b"bleprobe status"));
    assert!(!parse_ble_probe_start_command(b"BLEPROBE START 20"));
    assert!(!parse_ble_probe_status_command(b"BLEPROBE STOP"));
}

#[test]
fn parses_state_set_forms() {
    assert!(matches!(
        parse_state_set_command(b"STATE SET upload=ON"),
        Some(StateSetOperation::Upload(true))
    ));
}

#[test]
fn parses_diag_get_forms() {
    assert!(parse_diag_get_command(b"DIAG"));
    assert!(parse_diag_get_command(b"DIAG GET"));
    assert!(parse_diag_get_command(b"DIAG STATUS"));
    assert!(!parse_diag_get_command(b"DIAG START"));
}

#[test]
fn rejects_invalid_state_set_pairs() {
    assert!(parse_state_set_command(b"STATE SET foo=bar").is_none());
    assert!(parse_state_set_command(b"STATE SET overlay=CLOCK").is_none());
    assert!(parse_state_set_command(b"STATE SET upload=maybe").is_none());
    assert!(parse_state_set_command(b"STATE SET assets=off").is_none());
}

#[test]
fn parses_state_diag_and_targets() {
    let (kind, targets) =
        parse_state_diag_command(b"STATE DIAG kind=DEBUG targets=SD|WIFI").expect("diag parse");
    assert!(matches!(kind, DiagKind::Debug));
    assert_eq!(targets.as_persisted(), (1 << 0) | (1 << 1));

    let (none_kind, none_targets) =
        parse_state_diag_command(b"STATE DIAG kind=NONE targets=NONE").expect("diag none");
    assert!(matches!(none_kind, DiagKind::None));
    assert_eq!(none_targets.as_persisted(), 0);
}

#[test]
fn rejects_invalid_state_diag_values() {
    assert!(parse_state_diag_command(b"STATE DIAG kind=BAD targets=SD").is_none());
    assert!(parse_state_diag_command(b"STATE DIAG kind=TEST targets=SD|GPS").is_none());
    assert!(parse_state_diag_command(b"STATE DIAG targets=SD").is_none());
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_netcfg_set_json() {
    let parsed = parse_netcfg_set_command(
        br#"NETCFG SET {"ssid":"Suprematic","password":"abc12345","connect_timeout_ms":28000,"dhcp_timeout_ms":22000,"pinned_dhcp_timeout_ms":48000}"#,
    )
    .expect("netcfg parse");
    let creds = parsed.credentials.expect("credentials");
    assert_eq!(&creds.ssid[..creds.ssid_len as usize], b"Suprematic");
    assert_eq!(&creds.password[..creds.password_len as usize], b"abc12345");
    assert_eq!(parsed.policy.connect_timeout_ms, 28_000);
    assert_eq!(parsed.policy.dhcp_timeout_ms, 22_000);
    assert_eq!(parsed.policy.pinned_dhcp_timeout_ms, 48_000);
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_netcfg_set_policy_only() {
    let parsed =
        parse_netcfg_set_command(br#"NETCFG SET {"connect_timeout_ms":31000,"rotate_auth_max":7}"#)
            .expect("policy parse");
    assert!(parsed.credentials.is_none());
    assert_eq!(parsed.policy.connect_timeout_ms, 31_000);
    assert_eq!(parsed.policy.rotate_auth_max, 7);
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_net_listener_command() {
    assert_eq!(parse_net_listener_command(b"NET LISTENER ON"), Some(true));
    assert_eq!(parse_net_listener_command(b"NET LISTENER OFF"), Some(false));
    assert_eq!(parse_net_listener_command(b"NET LISTENER maybe"), None);
}
