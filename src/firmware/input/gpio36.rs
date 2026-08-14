const TOUCH_CLASSIFICATION_WINDOW_MS: u64 = 96;
const TOUCH_CLASSIFICATION_MIN_PROBES: u8 = 4;
const WAKE_BUTTON_DEBOUNCE_MS: u64 = 250;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gpio36Action {
    Touch,
    WakeButtonPressed,
    WakeButtonReleased,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gpio36Mode {
    SharedWithTouch,
    #[allow(dead_code)]
    ButtonOnly,
}

#[derive(Clone, Copy, Debug)]
struct PendingAssertion {
    started_ms: u64,
    no_contact_probes: u8,
}

#[derive(Debug, Default)]
pub(crate) struct Gpio36Classifier {
    pending: Option<PendingAssertion>,
    last_wake_button_ms: Option<u64>,
    wake_button_active: bool,
}

impl Gpio36Classifier {
    pub(crate) const fn new() -> Self {
        Self {
            pending: None,
            last_wake_button_ms: None,
            wake_button_active: false,
        }
    }

    pub(crate) fn on_asserted(&mut self, now_ms: u64, mode: Gpio36Mode) -> Option<Gpio36Action> {
        if matches!(mode, Gpio36Mode::ButtonOnly) {
            self.pending = None;
            return self.accept_wake_button(now_ms);
        }

        if self.pending.is_none() {
            self.pending = Some(PendingAssertion {
                started_ms: now_ms,
                no_contact_probes: 0,
            });
        }
        None
    }

    pub(crate) fn observe_touch_probe(
        &mut self,
        now_ms: u64,
        contact_present: bool,
    ) -> Option<Gpio36Action> {
        let pending = self.pending.as_mut()?;
        if contact_present {
            self.pending = None;
            return Some(Gpio36Action::Touch);
        }

        pending.no_contact_probes = pending.no_contact_probes.saturating_add(1);
        let elapsed_ms = now_ms.saturating_sub(pending.started_ms);
        if elapsed_ms < TOUCH_CLASSIFICATION_WINDOW_MS
            || pending.no_contact_probes < TOUCH_CLASSIFICATION_MIN_PROBES
        {
            return None;
        }

        self.pending = None;
        self.accept_wake_button(now_ms)
    }

    pub(crate) fn on_released(&mut self) -> Option<Gpio36Action> {
        // A release before classification is ambiguous: it can be a short WAKE
        // tap or the touchscreen interrupt returning high. Do not label it as a
        // button release unless the low period was already accepted as WAKE.
        self.pending = None;
        if !self.wake_button_active {
            return None;
        }
        self.wake_button_active = false;
        Some(Gpio36Action::WakeButtonReleased)
    }

    pub(crate) fn cancel_pending(&mut self) {
        self.pending = None;
    }

    #[cfg_attr(test, allow(dead_code))]
    pub(crate) const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    fn accept_wake_button(&mut self, now_ms: u64) -> Option<Gpio36Action> {
        if self
            .last_wake_button_ms
            .is_some_and(|last_ms| now_ms.saturating_sub(last_ms) < WAKE_BUTTON_DEBOUNCE_MS)
        {
            return None;
        }
        self.last_wake_button_ms = Some(now_ms);
        self.wake_button_active = true;
        Some(Gpio36Action::WakeButtonPressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_classifies_shared_assertion_as_touch() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::SharedWithTouch),
            None
        );
        assert_eq!(
            classifier.observe_touch_probe(108, true),
            Some(Gpio36Action::Touch)
        );
    }

    #[test]
    fn zero_frames_wait_through_touch_dropout_window() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);

        assert_eq!(classifier.observe_touch_probe(108, false), None);
        assert_eq!(classifier.observe_touch_probe(132, false), None);
        assert_eq!(classifier.observe_touch_probe(164, false), None);
        assert_eq!(classifier.observe_touch_probe(195, false), None);
        assert_eq!(
            classifier.observe_touch_probe(196, false),
            Some(Gpio36Action::WakeButtonPressed)
        );
    }

    #[test]
    fn late_contact_still_wins_before_window_expires() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);

        assert_eq!(classifier.observe_touch_probe(108, false), None);
        assert_eq!(classifier.observe_touch_probe(140, false), None);
        assert_eq!(classifier.observe_touch_probe(190, false), None);
        assert_eq!(
            classifier.observe_touch_probe(195, true),
            Some(Gpio36Action::Touch)
        );
    }

    #[test]
    fn repeated_assertion_does_not_restart_pending_window() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        classifier.on_asserted(180, Gpio36Mode::SharedWithTouch);

        for now_ms in [184, 188, 192, 196] {
            let result = classifier.observe_touch_probe(now_ms, false);
            if now_ms < 196 {
                assert_eq!(result, None);
            } else {
                assert_eq!(result, Some(Gpio36Action::WakeButtonPressed));
            }
        }
    }

    #[test]
    fn button_only_mode_resolves_without_touch_probe() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed)
        );
    }

    #[test]
    fn button_debounce_does_not_hide_a_real_touch() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed)
        );
        assert_eq!(classifier.on_asserted(200, Gpio36Mode::ButtonOnly), None);

        classifier.on_asserted(210, Gpio36Mode::SharedWithTouch);
        assert_eq!(
            classifier.observe_touch_probe(218, true),
            Some(Gpio36Action::Touch)
        );
    }

    #[test]
    fn cancellation_prevents_uncertain_button_classification() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        classifier.cancel_pending();
        assert_eq!(classifier.observe_touch_probe(300, false), None);
    }

    #[test]
    fn classified_button_emits_release() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed)
        );
        assert_eq!(
            classifier.on_released(),
            Some(Gpio36Action::WakeButtonReleased)
        );
        assert_eq!(classifier.on_released(), None);
    }

    #[test]
    fn touch_release_is_not_labeled_as_button() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        assert_eq!(
            classifier.observe_touch_probe(108, true),
            Some(Gpio36Action::Touch)
        );
        assert_eq!(classifier.on_released(), None);
    }

    #[test]
    fn ambiguous_short_assertion_is_not_labeled_as_button() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        assert_eq!(classifier.on_released(), None);
        assert_eq!(classifier.observe_touch_probe(300, false), None);
    }
}
