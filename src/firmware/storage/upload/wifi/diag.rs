use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};

use crate::firmware::types::{WifiCredentials, WifiRuntimePolicy, WIFI_SSID_MAX};

use super::state::{NetFailureClass, NetState, RecoveryLadderStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NetConfigSnapshot {
    pub(crate) credentials_set: bool,
    pub(crate) ssid: [u8; WIFI_SSID_MAX],
    pub(crate) ssid_len: u8,
    pub(crate) policy: WifiRuntimePolicy,
}

static NET_STATE: AtomicU8 = AtomicU8::new(NetState::Idle as u8);
static NET_FAILURE_CLASS: AtomicU8 = AtomicU8::new(NetFailureClass::None as u8);
static NET_FAILURE_CODE: AtomicU8 = AtomicU8::new(0);
static NET_LADDER_STEP: AtomicU8 = AtomicU8::new(RecoveryLadderStep::RetrySame as u8);
static NET_ATTEMPT: AtomicU32 = AtomicU32::new(0);
static NET_UPTIME_MS: AtomicU32 = AtomicU32::new(0);

static NETCFG_CREDENTIALS_SET: AtomicBool = AtomicBool::new(false);
static NETCFG_SSID_LEN: AtomicU8 = AtomicU8::new(0);
static NETCFG_CONNECT_TIMEOUT_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_DHCP_TIMEOUT_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_PINNED_DHCP_TIMEOUT_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_LISTENER_TIMEOUT_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_SCAN_ACTIVE_MIN_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_SCAN_ACTIVE_MAX_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_SCAN_PASSIVE_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_RETRY_SAME_MAX: AtomicU8 = AtomicU8::new(0);
static NETCFG_ROTATE_CANDIDATE_MAX: AtomicU8 = AtomicU8::new(0);
static NETCFG_ROTATE_AUTH_MAX: AtomicU8 = AtomicU8::new(0);
static NETCFG_FULL_SCAN_RESET_MAX: AtomicU8 = AtomicU8::new(0);
static NETCFG_DRIVER_RESTART_MAX: AtomicU8 = AtomicU8::new(0);
static NETCFG_COOLDOWN_MS: AtomicU32 = AtomicU32::new(0);
static NETCFG_DRIVER_RESTART_BACKOFF_MS: AtomicU32 = AtomicU32::new(0);
static mut NETCFG_SSID: [u8; WIFI_SSID_MAX] = [0; WIFI_SSID_MAX];

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

pub(super) fn publish_config(credentials: Option<WifiCredentials>, policy: WifiRuntimePolicy) {
    NETCFG_CREDENTIALS_SET.store(credentials.is_some(), Ordering::Relaxed);
    if let Some(credentials) = credentials {
        NETCFG_SSID_LEN.store(credentials.ssid_len, Ordering::Relaxed);
        let len = credentials.ssid_len as usize;
        // Safety: single writer in wifi task; readers copy atomically-bounded length.
        unsafe {
            let dst = core::ptr::addr_of_mut!(NETCFG_SSID) as *mut u8;
            core::ptr::write_bytes(dst, 0, WIFI_SSID_MAX);
            core::ptr::copy_nonoverlapping(credentials.ssid.as_ptr(), dst, len);
        }
    } else {
        NETCFG_SSID_LEN.store(0, Ordering::Relaxed);
        // Safety: single writer in wifi task.
        unsafe {
            let dst = core::ptr::addr_of_mut!(NETCFG_SSID) as *mut u8;
            core::ptr::write_bytes(dst, 0, WIFI_SSID_MAX);
        }
    }

    NETCFG_CONNECT_TIMEOUT_MS.store(policy.connect_timeout_ms, Ordering::Relaxed);
    NETCFG_DHCP_TIMEOUT_MS.store(policy.dhcp_timeout_ms, Ordering::Relaxed);
    NETCFG_PINNED_DHCP_TIMEOUT_MS.store(policy.pinned_dhcp_timeout_ms, Ordering::Relaxed);
    NETCFG_LISTENER_TIMEOUT_MS.store(policy.listener_timeout_ms, Ordering::Relaxed);
    NETCFG_SCAN_ACTIVE_MIN_MS.store(policy.scan_active_min_ms, Ordering::Relaxed);
    NETCFG_SCAN_ACTIVE_MAX_MS.store(policy.scan_active_max_ms, Ordering::Relaxed);
    NETCFG_SCAN_PASSIVE_MS.store(policy.scan_passive_ms, Ordering::Relaxed);
    NETCFG_RETRY_SAME_MAX.store(policy.retry_same_max, Ordering::Relaxed);
    NETCFG_ROTATE_CANDIDATE_MAX.store(policy.rotate_candidate_max, Ordering::Relaxed);
    NETCFG_ROTATE_AUTH_MAX.store(policy.rotate_auth_max, Ordering::Relaxed);
    NETCFG_FULL_SCAN_RESET_MAX.store(policy.full_scan_reset_max, Ordering::Relaxed);
    NETCFG_DRIVER_RESTART_MAX.store(policy.driver_restart_max, Ordering::Relaxed);
    NETCFG_COOLDOWN_MS.store(policy.cooldown_ms, Ordering::Relaxed);
    NETCFG_DRIVER_RESTART_BACKOFF_MS.store(policy.driver_restart_backoff_ms, Ordering::Relaxed);
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

pub(crate) fn net_config_snapshot() -> NetConfigSnapshot {
    let mut ssid = [0u8; WIFI_SSID_MAX];
    let ssid_len = NETCFG_SSID_LEN
        .load(Ordering::Relaxed)
        .min(WIFI_SSID_MAX as u8);
    let len = ssid_len as usize;
    // Safety: readers copy bounded bytes; single-writer updates from wifi task.
    unsafe {
        let src = core::ptr::addr_of!(NETCFG_SSID) as *const u8;
        core::ptr::copy_nonoverlapping(src, ssid.as_mut_ptr(), len);
    }
    NetConfigSnapshot {
        credentials_set: NETCFG_CREDENTIALS_SET.load(Ordering::Relaxed),
        ssid,
        ssid_len,
        policy: WifiRuntimePolicy {
            connect_timeout_ms: NETCFG_CONNECT_TIMEOUT_MS.load(Ordering::Relaxed),
            dhcp_timeout_ms: NETCFG_DHCP_TIMEOUT_MS.load(Ordering::Relaxed),
            pinned_dhcp_timeout_ms: NETCFG_PINNED_DHCP_TIMEOUT_MS.load(Ordering::Relaxed),
            listener_timeout_ms: NETCFG_LISTENER_TIMEOUT_MS.load(Ordering::Relaxed),
            scan_active_min_ms: NETCFG_SCAN_ACTIVE_MIN_MS.load(Ordering::Relaxed),
            scan_active_max_ms: NETCFG_SCAN_ACTIVE_MAX_MS.load(Ordering::Relaxed),
            scan_passive_ms: NETCFG_SCAN_PASSIVE_MS.load(Ordering::Relaxed),
            retry_same_max: NETCFG_RETRY_SAME_MAX.load(Ordering::Relaxed),
            rotate_candidate_max: NETCFG_ROTATE_CANDIDATE_MAX.load(Ordering::Relaxed),
            rotate_auth_max: NETCFG_ROTATE_AUTH_MAX.load(Ordering::Relaxed),
            full_scan_reset_max: NETCFG_FULL_SCAN_RESET_MAX.load(Ordering::Relaxed),
            driver_restart_max: NETCFG_DRIVER_RESTART_MAX.load(Ordering::Relaxed),
            cooldown_ms: NETCFG_COOLDOWN_MS.load(Ordering::Relaxed),
            driver_restart_backoff_ms: NETCFG_DRIVER_RESTART_BACKOFF_MS.load(Ordering::Relaxed),
        },
    }
}
