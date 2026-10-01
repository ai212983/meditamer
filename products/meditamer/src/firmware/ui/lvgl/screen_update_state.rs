#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PendingFastClock<Token> {
    None,
    AwaitingAdmission(Token),
    AwaitingPush(Token),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FastUpdateOperation {
    StrictFast,
    Clean,
    Wait,
}

/// Resolve a Fast request against co-ready Clean work and current panel
/// eligibility. A deferred Clean leaves a ready Fast request executable;
/// missing partial state promotes Fast only when Clean is currently allowed.
pub(crate) const fn select_fast_update_operation(
    clean_pending: bool,
    clean_allowed: bool,
    partial_ready: bool,
) -> FastUpdateOperation {
    if clean_pending && clean_allowed {
        FastUpdateOperation::Clean
    } else if partial_ready {
        FastUpdateOperation::StrictFast
    } else if clean_allowed {
        FastUpdateOperation::Clean
    } else {
        FastUpdateOperation::Wait
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ClockRemovalState<Token> {
    None,
    Requested(Token),
    Staged(Token),
}

impl<Token: Copy> ClockRemovalState<Token> {
    pub(super) const fn requested(self) -> Option<Token> {
        match self {
            Self::Requested(token) => Some(token),
            Self::None | Self::Staged(_) => None,
        }
    }

    pub(super) const fn staged(self) -> Option<Token> {
        match self {
            Self::Staged(token) => Some(token),
            Self::None | Self::Requested(_) => None,
        }
    }

    pub(super) fn request(&mut self, token: Token) {
        if matches!(*self, Self::None) {
            *self = Self::Requested(token);
        }
    }

    pub(super) fn stage(&mut self, token: Token) {
        if !matches!(*self, Self::Staged(_)) {
            *self = Self::Staged(token);
        }
    }

    pub(super) fn retain_requested(&mut self, overlay: Option<Token>)
    where
        Token: Eq,
    {
        if let Self::Requested(token) = *self {
            if overlay != Some(token) {
                *self = Self::None;
            }
        }
    }

    pub(super) fn take_staged(&mut self) -> Option<Token> {
        let token = self.staged()?;
        *self = Self::None;
        Some(token)
    }
}

impl<Token: Copy + Eq> PendingFastClock<Token> {
    pub(super) const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub(super) fn retain_current(&mut self, active_surface: Option<Token>, overlay: Option<Token>) {
        let current = match *self {
            Self::None => true,
            Self::AwaitingAdmission(token) => active_surface == Some(token),
            Self::AwaitingPush(token) => overlay == Some(token),
        };
        if !current {
            *self = Self::None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        select_fast_update_operation, ClockRemovalState, FastUpdateOperation, PendingFastClock,
    };

    #[test]
    fn admission_expires_with_its_surface_instance() {
        let mut pending = PendingFastClock::AwaitingAdmission(7_u32);
        pending.retain_current(Some(8), None);
        assert!(pending.is_none());
    }

    #[test]
    fn push_expires_with_its_overlay_instance() {
        let mut pending = PendingFastClock::AwaitingPush(7_u32);
        pending.retain_current(Some(7), Some(8));
        assert!(pending.is_none());
    }

    #[test]
    fn matching_instances_retain_pending_work() {
        let mut admission = PendingFastClock::AwaitingAdmission(7_u32);
        admission.retain_current(Some(7), None);
        assert!(!admission.is_none());

        let mut push = PendingFastClock::AwaitingPush(9_u32);
        push.retain_current(Some(7), Some(9));
        assert!(!push.is_none());
    }

    #[test]
    fn removal_stages_before_exactly_one_commit() {
        let mut removal = ClockRemovalState::None;
        removal.request(7_u32);
        assert_eq!(removal.requested(), Some(7));
        assert_eq!(removal.staged(), None);

        removal.stage(7);
        assert_eq!(removal.requested(), None);
        assert_eq!(removal.staged(), Some(7));
        assert_eq!(removal.take_staged(), Some(7));
        assert_eq!(removal.take_staged(), None);
    }

    #[test]
    fn removal_request_expires_with_its_overlay_instance() {
        let mut removal = ClockRemovalState::Requested(7_u32);
        removal.retain_requested(Some(8));
        assert_eq!(removal.requested(), None);
    }

    #[test]
    fn co_ready_clean_collapses_fast_into_one_clean_operation() {
        assert_eq!(
            select_fast_update_operation(true, true, true),
            FastUpdateOperation::Clean
        );
    }

    #[test]
    fn deferred_clean_leaves_ready_fast_work_executable() {
        assert_eq!(
            select_fast_update_operation(true, false, true),
            FastUpdateOperation::StrictFast
        );
    }

    #[test]
    fn unavailable_partial_state_waits_or_uses_allowed_clean_fallback() {
        assert_eq!(
            select_fast_update_operation(false, false, false),
            FastUpdateOperation::Wait
        );
        assert_eq!(
            select_fast_update_operation(false, true, false),
            FastUpdateOperation::Clean
        );
    }
}
