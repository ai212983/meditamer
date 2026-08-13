#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PanelPowerLeasePolicy {
    idle_ms: u64,
}

const CONFIGURED_IDLE_MS: u64 = 3_000;

impl PanelPowerLeasePolicy {
    pub(crate) const fn new(idle_ms: u64) -> Self {
        Self { idle_ms }
    }

    pub(crate) const fn configured() -> Self {
        Self::new(CONFIGURED_IDLE_MS)
    }

    pub(crate) const fn enabled(self) -> bool {
        self.idle_ms != 0
    }

    pub(crate) const fn idle_ms(self) -> u64 {
        self.idle_ms
    }

    pub(crate) const fn should_hold_terminal_state_for_partial(self) -> bool {
        self.enabled()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LeaseMaintenance {
    None,
    ShutDown { active_ms: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LeaseRefreshKind {
    Full,
    Partial,
    NoChange,
}

pub(crate) const fn should_shutdown_parked_panel(
    terminal_hold_requested: bool,
    completed: LeaseRefreshKind,
    full_fallback: bool,
) -> bool {
    terminal_hold_requested && full_fallback && matches!(completed, LeaseRefreshKind::Full)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PanelPowerLease {
    policy: PanelPowerLeasePolicy,
    started_ms: u64,
    idle_deadline_ms: Option<u64>,
    panel_held: bool,
}

impl PanelPowerLease {
    pub(crate) const fn new(policy: PanelPowerLeasePolicy) -> Self {
        Self {
            policy,
            started_ms: 0,
            idle_deadline_ms: None,
            panel_held: false,
        }
    }

    pub(crate) const fn policy(&self) -> PanelPowerLeasePolicy {
        self.policy
    }

    pub(crate) fn record_refresh_success(&mut self, kind: LeaseRefreshKind, now_ms: u64) -> bool {
        match kind {
            LeaseRefreshKind::Full => {
                self.clear();
                false
            }
            LeaseRefreshKind::NoChange => false,
            LeaseRefreshKind::Partial => self.record_partial_success(now_ms),
        }
    }

    fn record_partial_success(&mut self, now_ms: u64) -> bool {
        if !self.policy.enabled() {
            self.clear();
            return false;
        }
        if !self.panel_held {
            self.started_ms = now_ms;
        }
        self.panel_held = true;
        self.idle_deadline_ms = Some(now_ms.saturating_add(self.policy.idle_ms));
        true
    }

    pub(crate) fn mark_panel_off(&mut self) {
        self.clear();
    }

    pub(crate) fn take_maintenance(&mut self, now_ms: u64) -> LeaseMaintenance {
        if !self.panel_held
            || !self
                .idle_deadline_ms
                .is_some_and(|deadline| now_ms >= deadline)
        {
            return LeaseMaintenance::None;
        }
        let active_ms = now_ms.saturating_sub(self.started_ms);
        self.clear();
        LeaseMaintenance::ShutDown { active_ms }
    }

    fn clear(&mut self) {
        self.started_ms = 0;
        self.idle_deadline_ms = None;
        self.panel_held = false;
    }
}
