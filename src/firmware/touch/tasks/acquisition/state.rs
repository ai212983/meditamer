pub(super) const IDLE_RECOVERY_MS: u64 = 250;
pub(super) const ACTIVE_CONTACT_POLL_MS: u64 = 8;
const ELAN_TOUCH_REPORT_HEADER: u8 = 0x5A;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ContactSamplingState {
    active: bool,
    last_trace_ms: u64,
}

impl ContactSamplingState {
    pub(super) const fn new() -> Self {
        Self {
            active: false,
            last_trace_ms: 0,
        }
    }

    pub(super) const fn poll_interval_ms(self) -> u64 {
        if self.active {
            ACTIVE_CONTACT_POLL_MS
        } else {
            IDLE_RECOVERY_MS
        }
    }

    pub(super) fn record_authoritative_count(&mut self, touch_count: u8) {
        self.active = touch_count > 0;
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
