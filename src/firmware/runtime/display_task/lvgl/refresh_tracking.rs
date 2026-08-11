#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompletedRefresh {
    Full,
    Partial,
    NoChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RefreshTracking {
    partial_count: u32,
    recovery_required: bool,
}

impl RefreshTracking {
    pub(crate) const fn new() -> Self {
        Self {
            partial_count: 0,
            recovery_required: false,
        }
    }

    pub(crate) const fn partial_count(self) -> u32 {
        self.partial_count
    }

    pub(crate) const fn recovery_required(self) -> bool {
        self.recovery_required
    }

    pub(crate) fn should_request_full(self, automatic_limit: Option<u32>) -> bool {
        self.recovery_required
            || automatic_limit.is_some_and(|limit| limit > 0 && self.partial_count >= limit)
    }

    pub(crate) fn record_success(&mut self, completed: CompletedRefresh) {
        match completed {
            CompletedRefresh::Full => {
                self.partial_count = 0;
                self.recovery_required = false;
            }
            CompletedRefresh::Partial => {
                self.partial_count = self.partial_count.saturating_add(1);
                self.recovery_required = false;
            }
            CompletedRefresh::NoChange => {}
        }
    }

    pub(crate) fn record_failure(&mut self) {
        self.partial_count = 0;
        self.recovery_required = true;
    }
}
