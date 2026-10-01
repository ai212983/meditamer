//! Product-level runtime power intent.
//!
//! The target owns the hardware operations for each mode. Product code only
//! decides which mode an active surface requires; in particular, the
//! hourglass never requests [`RuntimePowerMode::Sleep`] or
//! [`RuntimePowerMode::DeepSleep`].

use hourglass::model::SessionState;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimePowerMode {
    Normal,
    HighPerformance,
    Sleep,
    DeepSleep,
}

impl RuntimePowerMode {
    pub const fn for_hourglass_state(state: SessionState) -> RuntimePowerMode {
        match state {
            SessionState::Running => RuntimePowerMode::HighPerformance,
            SessionState::Ready | SessionState::Paused | SessionState::Complete => {
                RuntimePowerMode::Normal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hourglass_uses_high_performance_only_while_running() {
        assert_eq!(
            RuntimePowerMode::for_hourglass_state(SessionState::Running),
            RuntimePowerMode::HighPerformance
        );
        for state in [
            SessionState::Ready,
            SessionState::Paused,
            SessionState::Complete,
        ] {
            assert_eq!(
                RuntimePowerMode::for_hourglass_state(state),
                RuntimePowerMode::Normal
            );
        }
    }

    #[test]
    fn hourglass_policy_never_enters_sleep() {
        for state in [
            SessionState::Ready,
            SessionState::Running,
            SessionState::Paused,
            SessionState::Complete,
        ] {
            assert_ne!(
                RuntimePowerMode::for_hourglass_state(state),
                RuntimePowerMode::Sleep
            );
            assert_ne!(
                RuntimePowerMode::for_hourglass_state(state),
                RuntimePowerMode::DeepSleep
            );
        }
    }
}
