const CLASSIFIER_PROBE_MS: u64 = 8;
const IDLE_RECOVERY_MS: u64 = 250;
const RELEASE_CONFIRM_FALLBACK_MS: u64 = 120;

pub(super) struct AcquisitionTiming {
    classifier_probe_at_ms: Option<u64>,
    release_confirm_at_ms: Option<u64>,
    idle_recovery_at_ms: u64,
    contact_seen: bool,
}

impl AcquisitionTiming {
    pub(super) fn new(now_ms: u64) -> Self {
        Self {
            classifier_probe_at_ms: None,
            release_confirm_at_ms: None,
            idle_recovery_at_ms: now_ms.saturating_add(IDLE_RECOVERY_MS),
            contact_seen: false,
        }
    }

    pub(super) fn reset_contact(&mut self) {
        self.contact_seen = false;
        self.release_confirm_at_ms = None;
    }

    pub(super) fn on_asserted(&mut self, now_ms: u64) {
        self.classifier_probe_at_ms = Some(now_ms.saturating_add(CLASSIFIER_PROBE_MS));
    }

    pub(super) fn on_released(&mut self) {
        self.classifier_probe_at_ms = None;
    }

    pub(super) fn take_active_probe_due(
        &mut self,
        now_ms: u64,
        line_low: bool,
        touch_ready: bool,
        classifier_pending: bool,
    ) -> bool {
        let due = line_low
            && touch_ready
            && (classifier_pending || self.contact_seen)
            && self
                .classifier_probe_at_ms
                .is_some_and(|deadline| now_ms >= deadline);
        if due {
            self.classifier_probe_at_ms = Some(now_ms.saturating_add(CLASSIFIER_PROBE_MS));
        }
        due
    }

    pub(super) fn take_release_confirm_due(&mut self, now_ms: u64) -> bool {
        let due = self
            .release_confirm_at_ms
            .is_some_and(|deadline| now_ms >= deadline);
        if due {
            self.reset_contact();
        }
        due
    }

    pub(super) fn take_idle_recovery_due(&mut self, now_ms: u64, touch_ready: bool) -> bool {
        let due = touch_ready && !self.contact_seen && now_ms >= self.idle_recovery_at_ms;
        if due {
            self.idle_recovery_at_ms = now_ms.saturating_add(IDLE_RECOVERY_MS);
        }
        due
    }

    pub(super) fn record_sample(&mut self, now_ms: u64, touch_count: u8) {
        if touch_count > 0 {
            self.contact_seen = true;
            self.release_confirm_at_ms = None;
        } else if self.contact_seen && self.release_confirm_at_ms.is_none() {
            self.release_confirm_at_ms = Some(now_ms.saturating_add(RELEASE_CONFIRM_FALLBACK_MS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_zero_does_not_extend_release_confirmation() {
        let mut timing = AcquisitionTiming::new(0);
        timing.record_sample(10, 1);
        timing.record_sample(20, 0);
        timing.record_sample(40, 0);

        assert!(!timing.take_release_confirm_due(139));
        assert!(timing.take_release_confirm_due(140));
    }

    #[test]
    fn contact_cancels_pending_release_confirmation() {
        let mut timing = AcquisitionTiming::new(0);
        timing.record_sample(10, 1);
        timing.record_sample(20, 0);
        timing.record_sample(30, 1);

        assert!(!timing.take_release_confirm_due(200));
    }

    #[test]
    fn classifier_probe_rearms_at_fixed_cadence() {
        let mut timing = AcquisitionTiming::new(0);
        timing.on_asserted(10);

        assert!(!timing.take_active_probe_due(10, true, true, true));
        assert!(!timing.take_active_probe_due(17, true, true, true));
        assert!(timing.take_active_probe_due(18, true, true, true));
    }

    #[test]
    fn active_contact_keeps_sampling_after_classification_finishes() {
        let mut timing = AcquisitionTiming::new(0);
        timing.on_asserted(10);
        timing.record_sample(10, 1);

        assert!(!timing.take_active_probe_due(10, true, true, false));
        assert!(timing.take_active_probe_due(18, true, true, false));
    }

    #[test]
    fn released_line_cancels_classifier_probe() {
        let mut timing = AcquisitionTiming::new(0);
        timing.on_asserted(10);
        timing.on_released();

        assert!(!timing.take_active_probe_due(18, true, true, true));
    }

    #[test]
    fn idle_recovery_rearms_at_fixed_cadence() {
        let mut timing = AcquisitionTiming::new(0);

        assert!(!timing.take_idle_recovery_due(249, true));
        assert!(timing.take_idle_recovery_due(250, true));
        assert!(!timing.take_idle_recovery_due(499, true));
        assert!(timing.take_idle_recovery_due(500, true));
    }
}
