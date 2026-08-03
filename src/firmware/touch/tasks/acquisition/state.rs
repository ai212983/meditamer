pub(super) const IDLE_RECOVERY_MS: u64 = 250;
pub(super) const ACTIVE_CONTACT_POLL_MS: u64 = 8;
const ELAN_TOUCH_REPORT_HEADER: u8 = 0x5A;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ContactSamplingState {
    active: bool,
}

impl ContactSamplingState {
    pub(super) const fn new() -> Self {
        Self { active: false }
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
