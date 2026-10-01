//! Pure formatting and timeout policy for the Ambient Home clock overlay.
//!
//! This module has no LVGL or product dependency so the host UI harness can
//! compile it unchanged.

use core::ffi::CStr;

pub(crate) const DISPLAY_DURATION_MS: u64 = 10_000;
const SECONDS_PER_DAY: u32 = 86_400;

pub(crate) fn timeout_elapsed(shown_at_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(shown_at_ms) >= DISPLAY_DURATION_MS
}

pub(crate) fn displayed_minute(local_epoch_seconds: u32) -> u16 {
    ((local_epoch_seconds % SECONDS_PER_DAY) / 60) as u16
}

pub(crate) fn format_time_hm(buffer: &mut [u8; 6], local_epoch_seconds: u32) -> Option<&CStr> {
    let seconds_of_day = local_epoch_seconds % SECONDS_PER_DAY;
    let hours = seconds_of_day / 3_600;
    let minutes = (seconds_of_day % 3_600) / 60;
    buffer[0] = b'0' + (hours / 10) as u8;
    buffer[1] = b'0' + (hours % 10) as u8;
    buffer[2] = b':';
    buffer[3] = b'0' + (minutes / 10) as u8;
    buffer[4] = b'0' + (minutes % 10) as u8;
    buffer[5] = 0;
    CStr::from_bytes_with_nul(buffer).ok()
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn formats_local_time_as_twenty_four_hour_hours_and_minutes() {
        let mut buffer = [0; 6];
        assert_eq!(
            format_time_hm(&mut buffer, 0).and_then(|value| value.to_str().ok()),
            Some("00:00")
        );
        assert_eq!(
            format_time_hm(&mut buffer, 86_399).and_then(|value| value.to_str().ok()),
            Some("23:59")
        );
    }

    #[test]
    fn wraps_epoch_seconds_at_local_midnight() {
        let mut buffer = [0; 6];
        assert_eq!(
            format_time_hm(&mut buffer, 86_400 + 5 * 3_600 + 7 * 60)
                .and_then(|value| value.to_str().ok()),
            Some("05:07")
        );
    }

    #[test]
    fn timeout_uses_saturating_monotonic_elapsed_time() {
        assert!(!timeout_elapsed(20_000, 19_999));
        assert!(!timeout_elapsed(20_000, 29_999));
        assert!(timeout_elapsed(20_000, 30_000));
    }
}
