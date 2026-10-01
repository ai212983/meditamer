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
pub struct NetStatusSnapshot {
    pub state: &'static str,
    pub link: bool,
    pub radio_quiesced: bool,
    pub ipv4: [u8; 4],
    pub listener: bool,
    pub listener_enabled: bool,
    pub failure_class: &'static str,
    pub failure_code: u8,
    pub ladder_step: &'static str,
    pub attempt: u32,
    pub uptime_ms: u32,
}

impl NetStatusSnapshot {
    /// Restoring the owner can finish while its connection task waits for
    /// provisioning. Recreating that task cannot supply missing credentials.
    /// This acknowledges service ownership, not network reachability; a
    /// configured connection still requires its link, lease, and listener.
    pub(crate) fn owner_restoration_ready(
        &self,
        upload_enabled: bool,
        credentials_set: bool,
    ) -> bool {
        let dormant = !upload_enabled && self.radio_quiesced;
        let awaiting_credentials = upload_enabled && !credentials_set;
        let idle = self.state == "Idle" && !self.link;
        if idle && (dormant || awaiting_credentials) {
            return true;
        }
        self.state == "Ready"
            && self.link
            && self.ipv4 != [0, 0, 0, 0]
            && (!self.listener_enabled || self.listener)
    }
}
