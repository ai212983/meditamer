#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NetState {
    Idle,
    Starting,
    Scanning,
    Associating,
    DhcpWait,
    ListenerWait,
    Ready,
    Recovering,
    Failed,
}

impl NetState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Starting => "Starting",
            Self::Scanning => "Scanning",
            Self::Associating => "Associating",
            Self::DhcpWait => "DhcpWait",
            Self::ListenerWait => "ListenerWait",
            Self::Ready => "Ready",
            Self::Recovering => "Recovering",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecoveryLadderStep {
    RetrySame,
    RotateCandidate,
    RotateAuth,
    FullScanReset,
    DriverRestart,
    TerminalFail,
}

impl RecoveryLadderStep {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RetrySame => "retry_same",
            Self::RotateCandidate => "rotate_candidate",
            Self::RotateAuth => "rotate_auth",
            Self::FullScanReset => "full_scan_reset",
            Self::DriverRestart => "driver_restart",
            Self::TerminalFail => "terminal_fail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NetFailureClass {
    None,
    ConnectTimeout,
    AuthReject,
    DiscoveryEmpty,
    DhcpNoIpv4,
    ListenerNotReady,
    PostRecoverStall,
    Transport,
    Unknown,
}

impl NetFailureClass {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ConnectTimeout => "connect_timeout",
            Self::AuthReject => "auth_reject",
            Self::DiscoveryEmpty => "discovery_empty",
            Self::DhcpNoIpv4 => "dhcp_no_ipv4",
            Self::ListenerNotReady => "listener_not_ready",
            Self::PostRecoverStall => "post_recover_stall",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NetStatusSnapshot {
    pub(crate) state: &'static str,
    pub(crate) link: bool,
    pub(crate) ipv4: [u8; 4],
    pub(crate) listener: bool,
    pub(crate) failure_class: &'static str,
    pub(crate) failure_code: u8,
    pub(crate) ladder_step: &'static str,
    pub(crate) attempt: u32,
    pub(crate) uptime_ms: u32,
}
