use core::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
};

use critical_section::Mutex;

use crate::config::{WifiCredentials, WifiRuntimePolicy, WIFI_SSID_MAX};

use super::state::{NetFailureClass, NetState, RecoveryLadderStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetConfigSnapshot {
    pub credentials_set: bool,
    pub ssid: [u8; WIFI_SSID_MAX],
    pub ssid_len: u8,
    pub policy: WifiRuntimePolicy,
}

impl NetConfigSnapshot {
    const EMPTY: Self = Self {
        credentials_set: false,
        ssid: [0; WIFI_SSID_MAX],
        ssid_len: 0,
        policy: WifiRuntimePolicy {
            connect_timeout_ms: 0,
            dhcp_timeout_ms: 0,
            pinned_dhcp_timeout_ms: 0,
            listener_timeout_ms: 0,
            scan_active_min_ms: 0,
            scan_active_max_ms: 0,
            scan_passive_ms: 0,
            retry_same_max: 0,
            rotate_candidate_max: 0,
            rotate_auth_max: 0,
            full_scan_reset_max: 0,
            driver_restart_max: 0,
            cooldown_ms: 0,
            driver_restart_backoff_ms: 0,
        },
    };

    fn from_config(credentials: Option<WifiCredentials>, policy: WifiRuntimePolicy) -> Self {
        let Some(credentials) = credentials else {
            return Self {
                policy,
                ..Self::EMPTY
            };
        };

        let ssid_len = credentials.ssid_len.min(WIFI_SSID_MAX as u8);
        let mut ssid = [0; WIFI_SSID_MAX];
        let len = ssid_len as usize;
        ssid[..len].copy_from_slice(&credentials.ssid[..len]);
        Self {
            credentials_set: true,
            ssid,
            ssid_len,
            policy,
        }
    }
}

static NET_STATE: AtomicU8 = AtomicU8::new(NetState::Idle as u8);
static NET_FAILURE_CLASS: AtomicU8 = AtomicU8::new(NetFailureClass::None as u8);
static NET_FAILURE_CODE: AtomicU8 = AtomicU8::new(0);
static NET_LADDER_STEP: AtomicU8 = AtomicU8::new(RecoveryLadderStep::RetrySame as u8);
static NET_ATTEMPT: AtomicU32 = AtomicU32::new(0);
static NET_UPTIME_MS: AtomicU32 = AtomicU32::new(0);
static RADIO_QUIESCED: AtomicBool = AtomicBool::new(false);

static NET_CONFIG: Mutex<Cell<NetConfigSnapshot>> = Mutex::new(Cell::new(NetConfigSnapshot::EMPTY));

fn replace_config_snapshot(
    cell: &Cell<NetConfigSnapshot>,
    credentials: Option<WifiCredentials>,
    policy: WifiRuntimePolicy,
) {
    cell.set(NetConfigSnapshot::from_config(credentials, policy));
}

pub(super) fn publish_state(
    state: NetState,
    ladder_step: RecoveryLadderStep,
    attempt: u32,
    failure_class: NetFailureClass,
    failure_code: u8,
    uptime_ms: u32,
) {
    NET_STATE.store(state as u8, Ordering::Relaxed);
    NET_LADDER_STEP.store(ladder_step as u8, Ordering::Relaxed);
    NET_ATTEMPT.store(attempt, Ordering::Relaxed);
    NET_FAILURE_CLASS.store(failure_class as u8, Ordering::Relaxed);
    NET_FAILURE_CODE.store(failure_code, Ordering::Relaxed);
    NET_UPTIME_MS.store(uptime_ms, Ordering::Relaxed);
}

pub(super) fn publish_radio_quiesced(quiesced: bool) {
    RADIO_QUIESCED.store(quiesced, Ordering::Release);
    arbitration::claim::set_radio_quiesced(quiesced);
}

pub(super) fn radio_quiesced() -> bool {
    RADIO_QUIESCED.load(Ordering::Acquire)
}

pub(super) fn publish_config(credentials: Option<WifiCredentials>, policy: WifiRuntimePolicy) {
    critical_section::with(|cs| {
        replace_config_snapshot(NET_CONFIG.borrow(cs), credentials, policy)
    });
}

pub(super) fn decode_state(raw: u8) -> NetState {
    match raw {
        0 => NetState::Idle,
        1 => NetState::Starting,
        2 => NetState::Scanning,
        3 => NetState::Associating,
        4 => NetState::DhcpWait,
        5 => NetState::ListenerWait,
        6 => NetState::Ready,
        7 => NetState::Recovering,
        8 => NetState::Failed,
        _ => NetState::Failed,
    }
}

pub(super) fn decode_ladder(raw: u8) -> RecoveryLadderStep {
    match raw {
        0 => RecoveryLadderStep::RetrySame,
        1 => RecoveryLadderStep::RotateCandidate,
        2 => RecoveryLadderStep::RotateAuth,
        3 => RecoveryLadderStep::FullScanReset,
        4 => RecoveryLadderStep::DriverRestart,
        5 => RecoveryLadderStep::TerminalFail,
        _ => RecoveryLadderStep::TerminalFail,
    }
}

pub(super) fn decode_failure_class(raw: u8) -> NetFailureClass {
    match raw {
        0 => NetFailureClass::None,
        1 => NetFailureClass::ConnectTimeout,
        2 => NetFailureClass::AuthReject,
        3 => NetFailureClass::DiscoveryEmpty,
        4 => NetFailureClass::DhcpNoIpv4,
        5 => NetFailureClass::ListenerNotReady,
        6 => NetFailureClass::PostRecoverStall,
        7 => NetFailureClass::Transport,
        _ => NetFailureClass::Unknown,
    }
}

pub(super) fn read_status_fields() -> (NetState, RecoveryLadderStep, u32, NetFailureClass, u8, u32)
{
    let state = decode_state(NET_STATE.load(Ordering::Relaxed));
    let ladder = decode_ladder(NET_LADDER_STEP.load(Ordering::Relaxed));
    let attempt = NET_ATTEMPT.load(Ordering::Relaxed);
    let failure_class = decode_failure_class(NET_FAILURE_CLASS.load(Ordering::Relaxed));
    let failure_code = NET_FAILURE_CODE.load(Ordering::Relaxed);
    let uptime_ms = NET_UPTIME_MS.load(Ordering::Relaxed);
    (
        state,
        ladder,
        attempt,
        failure_class,
        failure_code,
        uptime_ms,
    )
}

pub fn net_config_snapshot() -> NetConfigSnapshot {
    critical_section::with(|cs| NET_CONFIG.borrow(cs).get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_snapshot_publishes_identity_and_policy_together() {
        let mut credentials = WifiCredentials {
            ssid: [b'x'; WIFI_SSID_MAX],
            ssid_len: 3,
            password: [0; crate::config::WIFI_PASSWORD_MAX],
            password_len: 0,
        };
        credentials.ssid[..3].copy_from_slice(b"net");
        let policy = WifiRuntimePolicy {
            connect_timeout_ms: 1,
            dhcp_timeout_ms: 2,
            pinned_dhcp_timeout_ms: 3,
            listener_timeout_ms: 4,
            scan_active_min_ms: 5,
            scan_active_max_ms: 6,
            scan_passive_ms: 7,
            retry_same_max: 8,
            rotate_candidate_max: 9,
            rotate_auth_max: 10,
            full_scan_reset_max: 11,
            driver_restart_max: 12,
            cooldown_ms: 13,
            driver_restart_backoff_ms: 14,
        };

        let cell = Cell::new(NetConfigSnapshot::EMPTY);
        replace_config_snapshot(&cell, Some(credentials), policy);
        let snapshot = cell.get();

        assert!(snapshot.credentials_set);
        assert_eq!(snapshot.ssid_len, 3);
        assert_eq!(&snapshot.ssid[..3], b"net");
        assert!(snapshot.ssid[3..].iter().all(|byte| *byte == 0));
        assert_eq!(snapshot.policy, policy);
    }

    #[test]
    fn config_snapshot_bounds_an_invalid_public_length() {
        let credentials = WifiCredentials {
            ssid: [b'x'; WIFI_SSID_MAX],
            ssid_len: u8::MAX,
            password: [0; crate::config::WIFI_PASSWORD_MAX],
            password_len: 0,
        };

        let snapshot =
            NetConfigSnapshot::from_config(Some(credentials), WifiRuntimePolicy::defaults());

        assert!(snapshot.credentials_set);
        assert_eq!(snapshot.ssid_len, WIFI_SSID_MAX as u8);
        assert_eq!(snapshot.ssid, [b'x'; WIFI_SSID_MAX]);
    }

    #[test]
    fn config_snapshot_clears_absent_credentials_without_dropping_policy() {
        let policy = WifiRuntimePolicy::defaults();
        let cell = Cell::new(NetConfigSnapshot::EMPTY);
        replace_config_snapshot(&cell, None, policy);
        let snapshot = cell.get();

        assert!(!snapshot.credentials_set);
        assert_eq!(snapshot.ssid_len, 0);
        assert_eq!(snapshot.ssid, [0; WIFI_SSID_MAX]);
        assert_eq!(snapshot.policy, policy);
    }
}
