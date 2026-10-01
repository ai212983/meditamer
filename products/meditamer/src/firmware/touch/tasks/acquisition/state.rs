pub(super) const IDLE_RECOVERY_MS: u64 = 250;
pub(super) const ACTIVE_CONTACT_POLL_MS: u64 = 8;
const ELAN_TOUCH_REPORT_HEADER: u8 = 0x5A;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ContactSamplingState {
    active: bool,
    last_trace_ms: u64,
    last_read_start_ms: Option<u64>,
}

impl ContactSamplingState {
    pub(super) const fn new() -> Self {
        Self {
            active: false,
            last_trace_ms: 0,
            last_read_start_ms: None,
        }
    }

    pub(super) const fn poll_delay_ms(self, now_ms: u64) -> u64 {
        if self.active {
            match self.last_read_start_ms {
                Some(start_ms) => {
                    ACTIVE_CONTACT_POLL_MS.saturating_sub(now_ms.saturating_sub(start_ms))
                }
                None => ACTIVE_CONTACT_POLL_MS,
            }
        } else {
            IDLE_RECOVERY_MS
        }
    }

    pub(super) fn record_read_start(&mut self, t_ms: u64) {
        self.last_read_start_ms = Some(t_ms);
    }

    pub(super) fn record_authoritative_count(&mut self, touch_count: u8) {
        self.active = touch_count > 0;
        if !self.active {
            self.last_read_start_ms = None;
        }
    }

    /// Classifies controller output using contact context. ELAN normally emits
    /// a 0x5A report with zero active slots on release, but hardware traces also
    /// show exact all-zero packets after a valid active report. Accept that one
    /// unambiguous empty form only while a contact is already active; arbitrary
    /// non-touch packets and idle bus noise remain non-authoritative.
    pub(super) fn classify_touch_count(
        &self,
        raw: &[u8; 8],
        decoded_touch_count: u8,
    ) -> Option<u8> {
        authoritative_touch_count(raw[0], decoded_touch_count)
            .or_else(|| (self.active && raw.iter().all(|byte| *byte == 0)).then_some(0))
    }

    pub(super) fn should_trace(&mut self, now_ms: u64, authoritative_count: Option<u8>) -> bool {
        let state_changed = authoritative_count
            .map(|count| count > 0)
            .is_some_and(|observed_active| observed_active != self.active);
        let active_periodic = self.active && now_ms.saturating_sub(self.last_trace_ms) >= 64;
        if state_changed || active_periodic {
            self.last_trace_ms = now_ms;
            true
        } else {
            false
        }
    }
}

/// Returns a contact count only for an authoritative ELAN touch report.
/// Silence and unrelated controller packets must not synthesize a release.
pub(super) const fn authoritative_touch_count(
    report_header: u8,
    decoded_touch_count: u8,
) -> Option<u8> {
    if report_header == ELAN_TOUCH_REPORT_HEADER {
        Some(decoded_touch_count)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::ContactSamplingState;

    #[test]
    fn active_poll_delay_is_measured_from_previous_read_start() {
        let mut state = ContactSamplingState::new();
        assert_eq!(state.poll_delay_ms(100), 250);
        state.record_read_start(100);
        state.record_authoritative_count(1);
        assert_eq!(state.poll_delay_ms(103), 5);
        assert_eq!(state.poll_delay_ms(108), 0);
        assert_eq!(state.poll_delay_ms(116), 0);
        state.record_authoritative_count(0);
        assert_eq!(state.poll_delay_ms(116), 250);
    }
}
