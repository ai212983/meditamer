//! State and descriptor for Medinote's KEY-driven Counter app.

pub mod descriptor;

use flipclock::FlipClock;

/// Persistent Counter state: a press count for the label, and the flip clock
/// the screen renders.
///
/// The clock owns no time source. The target starts a flip on KEY and then
/// feeds it the runtime tick, which is what lets one press run a whole
/// animation instead of a single pose.
pub struct CounterState {
    count: u32,
    clock: FlipClock,
}

impl Default for CounterState {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterState {
    pub const fn new() -> Self {
        Self {
            count: 0,
            clock: FlipClock::new(0),
        }
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn clock(&self) -> &FlipClock {
        &self.clock
    }

    /// Starts one flip and wraps the count at `u32::MAX`.
    pub fn increment(&mut self) -> u32 {
        self.count = self.count.wrapping_add(1);
        self.clock.start();
        self.count
    }

    /// Advances the animation. Returns whether the screen needs repainting.
    pub fn advance(&mut self, delta_ms: u32) -> bool {
        self.clock.advance(delta_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero() {
        assert_eq!(CounterState::new().count(), 0);
    }

    /// One press is one whole flip: the clock starts animating and keeps
    /// asking for repaints until it settles.
    #[test]
    fn a_press_starts_an_animation_that_settles_on_its_own() {
        let mut state = CounterState::new();
        assert!(!state.clock().is_animating());
        assert!(!state.advance(33), "an idle clock asked for a repaint");

        state.increment();
        assert!(state.clock().is_animating());
        assert!(state.advance(33));
        for _ in 0..64 {
            state.advance(33);
        }
        assert!(!state.clock().is_animating(), "flip never settled");
    }

    #[test]
    fn increments_and_reports_the_new_value() {
        let mut state = CounterState::new();
        assert_eq!(state.increment(), 1);
        assert_eq!(state.increment(), 2);
        assert_eq!(state.count(), 2);
    }

    #[test]
    fn wraps_past_the_maximum_instead_of_panicking() {
        let mut state = CounterState {
            count: u32::MAX,
            ..CounterState::new()
        };
        assert_eq!(state.increment(), 0);
    }
}
