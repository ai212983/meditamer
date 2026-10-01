#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletedRefresh {
    Full,
    Partial,
    NoChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RefreshTracking {
    recovery_required: bool,
    startup_complete: bool,
    next_recovery_retry_ms: u64,
}

const RECOVERY_RETRY_INTERVAL_MS: u64 = 1_000;

impl RefreshTracking {
    pub(crate) const fn new() -> Self {
        Self {
            recovery_required: false,
            startup_complete: false,
            next_recovery_retry_ms: 0,
        }
    }

    pub(crate) const fn recovery_required(self) -> bool {
        self.recovery_required
    }

    pub(crate) const fn startup_complete(self) -> bool {
        self.startup_complete
    }

    /// A rejected initial scan must retry even when no new pixels become
    /// dirty. Back off from completion so persistent faults cannot spin.
    pub(crate) fn next_deadline_ms(&self) -> Option<u64> {
        self.recovery_required
            .then_some(self.next_recovery_retry_ms)
    }

    pub(crate) fn recovery_retry_due(self, now_ms: u64) -> bool {
        self.recovery_required && now_ms >= self.next_recovery_retry_ms
    }

    pub(crate) const fn should_request_full(self) -> bool {
        !self.startup_complete || self.recovery_required
    }

    pub(crate) fn record_success(&mut self, completed: CompletedRefresh) {
        match completed {
            CompletedRefresh::Full => {
                self.recovery_required = false;
                self.startup_complete = true;
            }
            CompletedRefresh::Partial => {
                self.recovery_required = false;
            }
            CompletedRefresh::NoChange => {}
        }
    }

    pub(crate) fn record_failure(&mut self, completed_ms: u64) {
        self.recovery_required = true;
        self.next_recovery_retry_ms = completed_ms.saturating_add(RECOVERY_RETRY_INTERVAL_MS);
    }
}

#[cfg(test)]
mod tests {
    use super::{CompletedRefresh, RefreshTracking};

    #[test]
    fn startup_requests_clean_refresh() {
        assert!(RefreshTracking::new().should_request_full());
    }

    #[test]
    fn partial_refreshes_do_not_accumulate_clean_refresh_debt() {
        let mut tracking = RefreshTracking::new();
        tracking.record_success(CompletedRefresh::Full);

        for _ in 0..100 {
            tracking.record_success(CompletedRefresh::Partial);
        }

        assert!(!tracking.should_request_full());
    }

    #[test]
    fn failed_refresh_requests_clean_recovery() {
        let mut tracking = RefreshTracking::new();
        tracking.record_success(CompletedRefresh::Full);
        assert!(!tracking.should_request_full());

        tracking.record_failure(5_000);

        assert!(tracking.should_request_full());
        assert!(!tracking.recovery_retry_due(5_999));
        assert!(tracking.recovery_retry_due(6_000));
    }
}
