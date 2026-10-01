use core::sync::atomic::{AtomicU32, Ordering};

const TOUCH_CLASSIFICATION_WINDOW_MS: u64 = 96;
const TOUCH_CLASSIFICATION_MIN_PROBES: u8 = 4;
const WAKE_BUTTON_DEBOUNCE_MS: u64 = 250;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gpio36Action {
    Touch,
    /// A classified WAKE press. `t_ms` is the source assertion timestamp and
    /// `generation` identifies this acceptance; the display (core 0) arms a
    /// pending press from it but decides short/long itself. Carrying the
    /// timestamp lets display-side deadline servicing stay correct when the
    /// event queue delivers the edge late.
    WakeButtonPressed {
        t_ms: u64,
        generation: u32,
    },
    WakeButtonReleased {
        t_ms: u64,
        generation: u32,
    },
}

/// Minimal cross-core snapshot of the accepted WAKE hold, published by the
/// acquisition task (core 1) and read by the display task (core 0). The
/// display compares the generation on an armed press against this snapshot
/// before toggling: a fault/suspend cancellation bumps the generation and
/// clears `active`, so a cancelled or otherwise stale hold can never toggle
/// later. Packed into one atomic so the display always loads a consistent
/// pair; `active == false` with a mismatched generation is the cancelled
/// state, so no third flag is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WakeHoldSnapshot {
    pub(crate) generation: u32,
    pub(crate) active: bool,
}

impl WakeHoldSnapshot {
    const fn pack(self) -> u32 {
        (self.generation << 1) | (self.active as u32)
    }

    const fn unpack(packed: u32) -> Self {
        Self {
            generation: packed >> 1,
            active: (packed & 1) != 0,
        }
    }
}

static WAKE_HOLD_SNAPSHOT: AtomicU32 = AtomicU32::new(
    WakeHoldSnapshot {
        generation: 0,
        active: false,
    }
    .pack(),
);

/// Mirrors the classifier's current acceptance into the cross-core snapshot.
/// Call after every classifier mutation that can change the accepted hold
/// (press accepted, release observed, accepted press cancelled) and before
/// publishing the corresponding edge, so a display-side staleness check
/// against the snapshot can never observe a newer edge with an older
/// snapshot.
pub(crate) fn sync_wake_snapshot(classifier: &Gpio36Classifier) {
    WAKE_HOLD_SNAPSHOT.store(classifier.snapshot().pack(), Ordering::Release);
}

pub(crate) fn load_wake_snapshot() -> WakeHoldSnapshot {
    WakeHoldSnapshot::unpack(WAKE_HOLD_SNAPSHOT.load(Ordering::Acquire))
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
    /// Acceptance generation. Bumped on every accepted press and on every
    /// [`Self::cancel_accepted`], so a press edge the display armed before a
    /// fault/suspend carries a generation that no longer matches the
    /// snapshot and can never toggle later.
    generation: u32,
    /// Set by [`Self::cancel_accepted`] when it ends a press this classifier
    /// had already accepted. While set, the line is treated as unreadable
    /// for classification purposes -- no new pending window starts, nothing
    /// resolves to a button press -- until [`Self::on_released`] observes
    /// the line actually go high, the same suppress-until-release shape
    /// `CaptureGate` holds for KEY/BOOT. This is what stops a hold that
    /// survives a fault or suspend from being relabeled a new WAKE press
    /// once classification resumes.
    suppressed_until_release: bool,
}

impl Gpio36Classifier {
    pub(crate) const fn new() -> Self {
        Self {
            pending: None,
            last_wake_button_ms: None,
            wake_button_active: false,
            generation: 0,
            suppressed_until_release: false,
        }
    }

    pub(crate) const fn snapshot(&self) -> WakeHoldSnapshot {
        WakeHoldSnapshot {
            generation: self.generation,
            active: self.wake_button_active,
        }
    }

    pub(crate) fn on_asserted(&mut self, now_ms: u64, mode: Gpio36Mode) -> Option<Gpio36Action> {
        if self.suppressed_until_release {
            return None;
        }
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

        let started_ms = pending.started_ms;
        self.pending = None;
        self.accept_wake_button(started_ms)
    }

    pub(crate) fn on_released(&mut self, now_ms: u64) -> Option<Gpio36Action> {
        // A release before classification is ambiguous: it can be a short WAKE
        // tap or the touchscreen interrupt returning high. Do not label it as a
        // button release unless the low period was already accepted as WAKE.
        self.pending = None;
        if self.suppressed_until_release {
            // The physical release that ends suppression -- not a real
            // button release, since nothing was accepted for it to end.
            self.suppressed_until_release = false;
            return None;
        }
        if !self.wake_button_active {
            return None;
        }
        self.wake_button_active = false;
        Some(Gpio36Action::WakeButtonReleased {
            t_ms: now_ms,
            generation: self.generation,
        })
    }

    pub(crate) fn cancel_pending(&mut self) {
        self.pending = None;
    }

    /// Ends a WAKE press this classifier had already accepted, if any,
    /// without producing a release -- for fault and suspend, where
    /// ownership must end but nothing consumed the accepted press as a real
    /// button click. Also clears any still-pending classification, same as
    /// [`Self::cancel_pending`]. Arms suppression only when a press was
    /// actually accepted: a still-pending classification alone carries no
    /// ownership to protect, so it can restart cleanly on the next
    /// assertion exactly as `cancel_pending` already allows. Always bumps
    /// the generation, so an already-published press edge the display armed
    /// before this cancellation can never match the snapshot again.
    pub(crate) fn cancel_accepted(&mut self) {
        self.pending = None;
        self.generation = next_generation(self.generation);
        if self.wake_button_active {
            self.wake_button_active = false;
            self.suppressed_until_release = true;
        }
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
        self.generation = next_generation(self.generation);
        Some(Gpio36Action::WakeButtonPressed {
            t_ms: now_ms,
            generation: self.generation,
        })
    }
}

/// The low packed-snapshot bit stores `active`, leaving 31 bits for the
/// generation. Wrapping is explicit and keeps every published generation
/// representable by the target-supported single `AtomicU32` snapshot.
const fn next_generation(generation: u32) -> u32 {
    generation.wrapping_add(1) & 0x7fff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_generation_wraps_within_31_bits() {
        assert_eq!(next_generation(0x7fff_fffe), 0x7fff_ffff);
        assert_eq!(next_generation(0x7fff_ffff), 0);
    }

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
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 100,
                generation: 1
            })
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
                assert_eq!(
                    result,
                    Some(Gpio36Action::WakeButtonPressed {
                        t_ms: 100,
                        generation: 1
                    })
                );
            }
        }
    }

    #[test]
    fn button_only_mode_resolves_without_touch_probe() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 100,
                generation: 1
            })
        );
    }

    #[test]
    fn button_debounce_does_not_hide_a_real_touch() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 100,
                generation: 1
            })
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
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 100,
                generation: 1
            })
        );
        assert_eq!(
            classifier.on_released(150),
            Some(Gpio36Action::WakeButtonReleased {
                t_ms: 150,
                generation: 1
            })
        );
        assert_eq!(classifier.on_released(160), None);
    }

    #[test]
    fn touch_release_is_not_labeled_as_button() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        assert_eq!(
            classifier.observe_touch_probe(108, true),
            Some(Gpio36Action::Touch)
        );
        assert_eq!(classifier.on_released(120), None);
    }

    #[test]
    fn ambiguous_short_assertion_is_not_labeled_as_button() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        assert_eq!(classifier.on_released(120), None);
        assert_eq!(classifier.observe_touch_probe(300, false), None);
    }

    /// A fault or suspend that fires while an accepted WAKE press is still
    /// held must not let it come back as a new press once classification
    /// resumes -- the button-recognizer counterpart is
    /// `capture_gate_cancels_on_revoke_and_suppresses_through_regrant_until_release`.
    #[test]
    fn cancel_accepted_suppresses_a_held_wake_button_until_release() {
        let mut classifier = Gpio36Classifier::new();
        assert_eq!(
            classifier.on_asserted(100, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 100,
                generation: 1
            })
        );

        // Fault/suspend fires with the button still held -- no release, no
        // re-press, just ownership ending.
        classifier.cancel_accepted();
        assert_eq!(classifier.snapshot().generation, 2);
        assert!(!classifier.snapshot().active);
        assert_eq!(classifier.on_released(150), None);

        // The line is still (or again) asserted once classification resumes
        // -- this must not be read as a new press.
        assert_eq!(classifier.on_asserted(200, Gpio36Mode::ButtonOnly), None);
        assert_eq!(classifier.on_asserted(300, Gpio36Mode::ButtonOnly), None);

        // Only the actual physical release ends suppression.
        assert_eq!(classifier.on_released(350), None);

        // A fresh assertion after that release is a genuinely new press.
        assert_eq!(
            classifier.on_asserted(2_000, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 2_000,
                generation: 3
            })
        );
    }

    /// `cancel_accepted` on a classifier with nothing accepted (idle, or
    /// only a pending classification) behaves exactly like `cancel_pending`
    /// -- no suppression armed, since there is no ownership to protect.
    #[test]
    fn cancel_accepted_without_an_accepted_press_does_not_suppress() {
        let mut classifier = Gpio36Classifier::new();
        classifier.on_asserted(100, Gpio36Mode::SharedWithTouch);
        classifier.cancel_accepted();
        assert_eq!(classifier.observe_touch_probe(300, false), None);

        assert_eq!(
            classifier.on_asserted(400, Gpio36Mode::ButtonOnly),
            Some(Gpio36Action::WakeButtonPressed {
                t_ms: 400,
                generation: 2
            })
        );
    }
}
