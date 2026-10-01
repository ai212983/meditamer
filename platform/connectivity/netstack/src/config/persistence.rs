//! Installed credential-store port and shared network status formatting.
//!
//! The target owns the physical flash singleton and installs one synchronous
//! store function during boot. Keeping the port synchronous avoids another
//! resident Embassy task and task-pool allocation; the target flash owner is
//! still the only place that can serialize physical flash access.

use core::cell::RefCell;
use core::fmt::Write as _;

use critical_section::Mutex;

use super::types::{WifiConfigResultCode, WifiCredentials};

type CredentialStore = fn(&WifiCredentials) -> WifiConfigResultCode;

static CREDENTIAL_STORE: Mutex<RefCell<Option<CredentialStore>>> = Mutex::new(RefCell::new(None));

/// Install the target's sole internal-flash credential writer.
///
/// This is a boot-only operation. Replacing a live backend could split one
/// product's runtime state across two physical stores, so a second install is
/// rejected.
pub fn install_credential_store(store: CredentialStore) {
    critical_section::with(|cs| {
        let mut slot = CREDENTIAL_STORE.borrow_ref_mut(cs);
        assert!(slot.is_none(), "credential store installed twice");
        *slot = Some(store);
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreCredentialsError {
    /// No target flash backend was installed during boot.
    Unavailable,
    /// The installed backend reported a durable-write failure.
    Failed(WifiConfigResultCode),
}

/// Store through the target's internal-flash owner. Success means the new
/// record was written and read back successfully.
pub fn store_credentials(credentials: &WifiCredentials) -> Result<(), StoreCredentialsError> {
    let store = critical_section::with(|cs| *CREDENTIAL_STORE.borrow_ref(cs))
        .ok_or(StoreCredentialsError::Unavailable)?;
    match store(credentials) {
        WifiConfigResultCode::Ok => Ok(()),
        code => Err(StoreCredentialsError::Failed(code)),
    }
}

pub const fn result_code_label(code: WifiConfigResultCode) -> &'static str {
    match code {
        WifiConfigResultCode::Ok => "ok",
        WifiConfigResultCode::InvalidData => "invalid_data",
        WifiConfigResultCode::OperationFailed => "operation_failed",
    }
}

/// The `NETCFG {...}` status line the serial `NETCFG GET` command reports.
/// Pure formatting over [`crate::wifi::net_config_snapshot`]; the caller only
/// writes the returned bytes to its transport.
pub fn format_netcfg_status_line() -> heapless::String<512> {
    format_netcfg_snapshot(crate::wifi::net_config_snapshot())
}

/// Desired configuration, available even before a target starts its radio.
pub fn format_configured_netcfg_status_line() -> heapless::String<512> {
    let config = crate::wifi::current_runtime_config();
    let mut ssid = heapless::String::new();
    if let Some(credentials) = config.credentials {
        if let Ok(value) = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize])
        {
            let _ = ssid.push_str(value);
        }
    }
    format_netcfg_snapshot(crate::wifi::NetConfigSnapshotView {
        credentials_set: config.credentials.is_some(),
        ssid,
        policy: config.policy,
    })
}

fn format_netcfg_snapshot(snapshot: crate::wifi::NetConfigSnapshotView) -> heapless::String<512> {
    let mut line = heapless::String::<512>::new();
    let _ = write!(
        &mut line,
        "NETCFG {{\"ssid_set\":{},\"ssid\":\"{}\",\"policy\":{{\"connect_timeout_ms\":{},\"dhcp_timeout_ms\":{},\"pinned_dhcp_timeout_ms\":{},\"listener_timeout_ms\":{},\"scan_active_min_ms\":{},\"scan_active_max_ms\":{},\"scan_passive_ms\":{},\"retry_same_max\":{},\"rotate_candidate_max\":{},\"rotate_auth_max\":{},\"full_scan_reset_max\":{},\"driver_restart_max\":{},\"cooldown_ms\":{},\"driver_restart_backoff_ms\":{}}}}}\r\n",
        if snapshot.credentials_set { "true" } else { "false" },
        snapshot.ssid,
        snapshot.policy.connect_timeout_ms,
        snapshot.policy.dhcp_timeout_ms,
        snapshot.policy.pinned_dhcp_timeout_ms,
        snapshot.policy.listener_timeout_ms,
        snapshot.policy.scan_active_min_ms,
        snapshot.policy.scan_active_max_ms,
        snapshot.policy.scan_passive_ms,
        snapshot.policy.retry_same_max,
        snapshot.policy.rotate_candidate_max,
        snapshot.policy.rotate_auth_max,
        snapshot.policy.full_scan_reset_max,
        snapshot.policy.driver_restart_max,
        snapshot.policy.cooldown_ms,
        snapshot.policy.driver_restart_backoff_ms,
    );
    line
}

/// Shared operational status wire format for both target consoles.
pub fn format_net_status_line() -> heapless::String<320> {
    let status = crate::wifi::net_status_snapshot();
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        &mut line,
        "NET_STATUS {{\"state\":\"{}\",\"link\":{},\"radio_quiesced\":{},\"ipv4\":\"{}.{}.{}.{}\",\"listener\":{},\"listener_enabled\":{},\"failure_class\":\"{}\",\"failure_code\":{},\"ladder_step\":\"{}\",\"attempt\":{},\"uptime_ms\":{}}}\r\n",
        status.state,
        if status.link { "true" } else { "false" },
        if status.radio_quiesced { "true" } else { "false" },
        status.ipv4[0],
        status.ipv4[1],
        status.ipv4[2],
        status.ipv4[3],
        if status.listener { "true" } else { "false" },
        if status.listener_enabled { "true" } else { "false" },
        status.failure_class,
        status.failure_code,
        status.ladder_step,
        status.attempt,
        status.uptime_ms,
    );
    line
}
