//! ADR-0011's diagnostic-window deadline state machine (shared BLE runtime
//! and roles plan, Phase 4 increment 5: "ADR-0011's deadline logic... as
//! host-testable, clock-free logic").
//!
//! ADR-0011: "A validated configuration sets these deadlines; current
//! defaults are: advertising: 60 seconds; connection: 60 seconds, with a
//! 30-second idle timeout; complete visibility window: 120 seconds;
//! teardown acknowledgement: two seconds. All values are positive; idle is
//! at most connection, and advertising and connection are at most the
//! complete window." The teardown-acknowledgement deadline is not this
//! module's concern -- it is already the existing `ble-foundation` probe's
//! `CALLBACK_QUIESCENCE_TIMEOUT` (2 s), unchanged since before this plan
//! started; this module owns the other three, which govern *when a window
//! must close*, not the already-correct fencing that follows once it does.
//!
//! [`VisibilityWindow`] tracks one window's phase (`Advertising` ->
//! `Connected`/`ConnectedIdle` -> `Closing`) against [`DeadlineConfig`]'s
//! bounds. Host-testable and clock-free like the rest of this crate:
//! callers supply `now_ticks` and their own tick unit (`DeadlineConfig`'s
//! constructors take `one_second_ticks` for exactly that reason, matching
//! [`crate::diagnostic::EchoRateLimiter`]'s `one_second_ticks` parameter).

/// One window's deadline bounds, in the caller's own tick unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadlineConfig {
    pub advertising_ticks: u64,
    pub connection_ticks: u64,
    pub idle_ticks: u64,
    pub window_ticks: u64,
}

/// Why a [`DeadlineConfig`] does not satisfy ADR-0011's own stated
/// invariants ("All values are positive; idle is at most connection, and
/// advertising and connection are at most the complete window").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeadlineConfigError {
    NotPositive,
    IdleExceedsConnection,
    AdvertisingExceedsWindow,
    ConnectionExceedsWindow,
}

impl DeadlineConfig {
    /// ADR-0011's own stated defaults (60 s advertising, 60 s connection
    /// with a 30 s idle timeout, 120 s complete window), expressed in the
    /// caller's tick unit via `one_second_ticks`.
    pub const fn adr_0011_defaults(one_second_ticks: u64) -> Self {
        Self {
            advertising_ticks: one_second_ticks.saturating_mul(60),
            connection_ticks: one_second_ticks.saturating_mul(60),
            idle_ticks: one_second_ticks.saturating_mul(30),
            window_ticks: one_second_ticks.saturating_mul(120),
        }
    }

    pub const fn validate(&self) -> Result<(), DeadlineConfigError> {
        if self.advertising_ticks == 0
            || self.connection_ticks == 0
            || self.idle_ticks == 0
            || self.window_ticks == 0
        {
            return Err(DeadlineConfigError::NotPositive);
        }
        if self.idle_ticks > self.connection_ticks {
            return Err(DeadlineConfigError::IdleExceedsConnection);
        }
        if self.advertising_ticks > self.window_ticks {
            return Err(DeadlineConfigError::AdvertisingExceedsWindow);
        }
        if self.connection_ticks > self.window_ticks {
            return Err(DeadlineConfigError::ConnectionExceedsWindow);
        }
        Ok(())
    }
}

/// A window's current phase. Mirrors [`crate::diagnostic::LifecycleState`]
/// one-to-one (see [`VisibilityWindow::lifecycle_state`]) but stays a
/// separate type: this one is this module's own state, `LifecycleState` is
/// the wire encoding of it, and the two should not be conflated even though
/// they currently agree exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPhase {
    Advertising,
    Connected,
    ConnectedIdle,
    Closing,
}

/// Why [`VisibilityWindow::check_deadline`] says the window must close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCloseReason {
    /// No connection arrived within the advertising deadline.
    AdvertisingTimeout,
    /// The connection deadline elapsed, connected or not.
    ConnectionTimeout,
    /// No GATT activity for the idle deadline while connected -- ADR-0011's
    /// `ConnectedIdle` is reached and closure requested in the same tick
    /// (see this module's doc comment on why there is no separate
    /// idle-then-later-close phase).
    IdleTimeout,
    /// The complete window deadline elapsed, regardless of phase.
    WindowTimeout,
}

/// One diagnostic window's phase and deadline tracking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityWindow {
    config: DeadlineConfig,
    phase: WindowPhase,
    window_start: u64,
    phase_start: u64,
    last_activity: u64,
}

impl VisibilityWindow {
    /// Opens a new window in `Advertising` phase at `now_ticks`. Rejects an
    /// invalid `config` up front rather than misbehaving partway through a
    /// window.
    pub fn open(config: DeadlineConfig, now_ticks: u64) -> Result<Self, DeadlineConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            phase: WindowPhase::Advertising,
            window_start: now_ticks,
            phase_start: now_ticks,
            last_activity: now_ticks,
        })
    }

    pub const fn phase(&self) -> WindowPhase {
        self.phase
    }

    /// This window's phase, encoded as the wire-facing
    /// [`crate::diagnostic::LifecycleState`].
    pub const fn lifecycle_state(&self) -> crate::diagnostic::LifecycleState {
        match self.phase {
            WindowPhase::Advertising => crate::diagnostic::LifecycleState::Advertising,
            WindowPhase::Connected => crate::diagnostic::LifecycleState::Connected,
            WindowPhase::ConnectedIdle => crate::diagnostic::LifecycleState::ConnectedIdle,
            WindowPhase::Closing => crate::diagnostic::LifecycleState::Closing,
        }
    }

    /// A connection was accepted: moves to `Connected` and resets both the
    /// connection deadline and the idle clock to start from `now_ticks`.
    pub fn on_connected(&mut self, now_ticks: u64) {
        self.phase = WindowPhase::Connected;
        self.phase_start = now_ticks;
        self.last_activity = now_ticks;
    }

    /// GATT traffic occurred (ADR-0011: "Connection churn, GATT traffic,
    /// CCC writes, notifications, and callbacks preserve the configured
    /// deadlines"): resets the idle clock, and recovers from `ConnectedIdle`
    /// back to `Connected` if the caller did not already act on a prior
    /// `IdleTimeout` close signal.
    pub fn on_activity(&mut self, now_ticks: u64) {
        self.last_activity = now_ticks;
        if self.phase == WindowPhase::ConnectedIdle {
            self.phase = WindowPhase::Connected;
        }
    }

    /// Forces `Closing` regardless of phase or deadlines (a disconnect, or
    /// an explicit close request).
    pub fn close(&mut self) {
        self.phase = WindowPhase::Closing;
    }

    /// Advances phase transitions implied by the passage of time and
    /// returns `Some(reason)` the first tick a deadline is reached; once
    /// closing, always returns `None` (the caller is expected to have
    /// already acted on the first `Some`).
    pub fn check_deadline(&mut self, now_ticks: u64) -> Option<WindowCloseReason> {
        if matches!(self.phase, WindowPhase::Closing) {
            return None;
        }
        if now_ticks.saturating_sub(self.window_start) >= self.config.window_ticks {
            self.phase = WindowPhase::Closing;
            return Some(WindowCloseReason::WindowTimeout);
        }
        match self.phase {
            WindowPhase::Advertising => {
                if now_ticks.saturating_sub(self.phase_start) >= self.config.advertising_ticks {
                    self.phase = WindowPhase::Closing;
                    return Some(WindowCloseReason::AdvertisingTimeout);
                }
                None
            }
            WindowPhase::Connected | WindowPhase::ConnectedIdle => {
                if now_ticks.saturating_sub(self.phase_start) >= self.config.connection_ticks {
                    self.phase = WindowPhase::Closing;
                    return Some(WindowCloseReason::ConnectionTimeout);
                }
                if now_ticks.saturating_sub(self.last_activity) >= self.config.idle_ticks {
                    self.phase = WindowPhase::ConnectedIdle;
                    return Some(WindowCloseReason::IdleTimeout);
                }
                None
            }
            WindowPhase::Closing => None,
        }
    }

    /// Ticks remaining until whichever deadline currently governs this
    /// window (the tighter of the phase-specific and complete-window
    /// bounds) -- for [`crate::diagnostic::LifecycleStatus`]'s
    /// `remaining_seconds` field. Zero once closing.
    pub fn remaining_ticks(&self, now_ticks: u64) -> u64 {
        let window_remaining = self
            .config
            .window_ticks
            .saturating_sub(now_ticks.saturating_sub(self.window_start));
        let phase_remaining = match self.phase {
            WindowPhase::Advertising => self
                .config
                .advertising_ticks
                .saturating_sub(now_ticks.saturating_sub(self.phase_start)),
            WindowPhase::Connected | WindowPhase::ConnectedIdle => self
                .config
                .connection_ticks
                .saturating_sub(now_ticks.saturating_sub(self.phase_start)),
            WindowPhase::Closing => 0,
        };
        window_remaining.min(phase_remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: u64 = 1_000_000;

    fn adr_config() -> DeadlineConfig {
        DeadlineConfig::adr_0011_defaults(SECOND)
    }

    #[test]
    fn adr_defaults_are_valid() {
        assert_eq!(adr_config().validate(), Ok(()));
        assert_eq!(adr_config().advertising_ticks, 60 * SECOND);
        assert_eq!(adr_config().connection_ticks, 60 * SECOND);
        assert_eq!(adr_config().idle_ticks, 30 * SECOND);
        assert_eq!(adr_config().window_ticks, 120 * SECOND);
    }

    #[test]
    fn zero_valued_field_is_rejected() {
        let config = DeadlineConfig {
            advertising_ticks: 0,
            ..adr_config()
        };
        assert_eq!(config.validate(), Err(DeadlineConfigError::NotPositive));
    }

    #[test]
    fn idle_exceeding_connection_is_rejected() {
        let config = DeadlineConfig {
            idle_ticks: 61 * SECOND,
            connection_ticks: 60 * SECOND,
            ..adr_config()
        };
        assert_eq!(
            config.validate(),
            Err(DeadlineConfigError::IdleExceedsConnection)
        );
    }

    #[test]
    fn advertising_exceeding_window_is_rejected() {
        let config = DeadlineConfig {
            advertising_ticks: 121 * SECOND,
            window_ticks: 120 * SECOND,
            ..adr_config()
        };
        assert_eq!(
            config.validate(),
            Err(DeadlineConfigError::AdvertisingExceedsWindow)
        );
    }

    #[test]
    fn connection_exceeding_window_is_rejected() {
        let config = DeadlineConfig {
            connection_ticks: 121 * SECOND,
            window_ticks: 120 * SECOND,
            ..adr_config()
        };
        assert_eq!(
            config.validate(),
            Err(DeadlineConfigError::ConnectionExceedsWindow)
        );
    }

    #[test]
    fn open_rejects_an_invalid_config() {
        let config = DeadlineConfig {
            advertising_ticks: 0,
            ..adr_config()
        };
        assert_eq!(
            VisibilityWindow::open(config, 0),
            Err(DeadlineConfigError::NotPositive)
        );
    }

    #[test]
    fn opens_advertising_and_stays_open_before_the_advertising_deadline() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        assert_eq!(window.phase(), WindowPhase::Advertising);
        assert_eq!(window.check_deadline(59 * SECOND), None);
        assert_eq!(window.phase(), WindowPhase::Advertising);
    }

    #[test]
    fn advertising_deadline_closes_the_window() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        assert_eq!(
            window.check_deadline(60 * SECOND),
            Some(WindowCloseReason::AdvertisingTimeout)
        );
        assert_eq!(window.phase(), WindowPhase::Closing);
        // Once closing, further checks report nothing further.
        assert_eq!(window.check_deadline(200 * SECOND), None);
    }

    #[test]
    fn connecting_resets_the_advertising_deadline_and_starts_the_connection_one() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(50 * SECOND);
        assert_eq!(window.phase(), WindowPhase::Connected);
        // Past the original 60s advertising deadline from t=0, but the
        // connection deadline only started at t=50s.
        assert_eq!(window.check_deadline(65 * SECOND), None);
    }

    #[test]
    fn connection_deadline_closes_the_window() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(0);
        assert_eq!(
            window.check_deadline(60 * SECOND),
            Some(WindowCloseReason::ConnectionTimeout)
        );
    }

    #[test]
    fn idle_deadline_transitions_to_connected_idle_and_requests_close() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(0);
        assert_eq!(
            window.check_deadline(30 * SECOND),
            Some(WindowCloseReason::IdleTimeout)
        );
        assert_eq!(window.phase(), WindowPhase::ConnectedIdle);
    }

    #[test]
    fn activity_resets_the_idle_clock_and_never_reaches_idle() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(0);
        window.on_activity(29 * SECOND);
        // Idle clock now starts at 29s, so just under 30s later (t=58s) is
        // not idle yet -- at exactly t=59s the 30s idle deadline would fire.
        assert_eq!(window.check_deadline(58 * SECOND), None);
        assert_eq!(window.phase(), WindowPhase::Connected);
    }

    #[test]
    fn activity_recovers_from_connected_idle() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(0);
        assert_eq!(
            window.check_deadline(30 * SECOND),
            Some(WindowCloseReason::IdleTimeout)
        );
        window.on_activity(31 * SECOND);
        assert_eq!(window.phase(), WindowPhase::Connected);
    }

    #[test]
    fn window_deadline_closes_regardless_of_phase() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(SECOND);
        window.on_activity(SECOND);
        assert_eq!(
            window.check_deadline(120 * SECOND),
            Some(WindowCloseReason::WindowTimeout)
        );
    }

    #[test]
    fn close_forces_closing_regardless_of_deadlines() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.close();
        assert_eq!(window.phase(), WindowPhase::Closing);
        assert_eq!(window.check_deadline(1), None);
    }

    #[test]
    fn lifecycle_state_matches_phase_one_to_one() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        assert_eq!(
            window.lifecycle_state(),
            crate::diagnostic::LifecycleState::Advertising
        );
        window.on_connected(0);
        assert_eq!(
            window.lifecycle_state(),
            crate::diagnostic::LifecycleState::Connected
        );
        window.check_deadline(30 * SECOND);
        assert_eq!(
            window.lifecycle_state(),
            crate::diagnostic::LifecycleState::ConnectedIdle
        );
        window.close();
        assert_eq!(
            window.lifecycle_state(),
            crate::diagnostic::LifecycleState::Closing
        );
    }

    #[test]
    fn remaining_ticks_counts_down_and_reaches_zero_at_the_deadline() {
        let window = VisibilityWindow::open(adr_config(), 0).unwrap();
        assert_eq!(window.remaining_ticks(0), 60 * SECOND);
        assert_eq!(window.remaining_ticks(59 * SECOND), SECOND);
        assert_eq!(window.remaining_ticks(60 * SECOND), 0);
    }

    #[test]
    fn remaining_ticks_uses_the_tighter_of_phase_and_window_bounds() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.on_connected(90 * SECOND);
        // Connection deadline from t=90s would allow up to t=150s, but the
        // complete window closes at t=120s regardless -- the window bound
        // must win.
        assert_eq!(window.remaining_ticks(100 * SECOND), 20 * SECOND);
    }

    #[test]
    fn remaining_ticks_is_zero_while_closing() {
        let mut window = VisibilityWindow::open(adr_config(), 0).unwrap();
        window.close();
        assert_eq!(window.remaining_ticks(0), 0);
    }
}
