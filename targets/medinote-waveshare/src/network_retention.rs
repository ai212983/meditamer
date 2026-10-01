//! Tiny RTC-retained network policy handoff for deep-sleep resets.
//!
//! The network owner remains authoritative while the chip is running. This
//! marker only carries the user's enabled policy across the reset caused by
//! deep sleep; it is consumed once during the following boot.

use core::sync::atomic::{AtomicBool, Ordering};

use esp_hal::rtc_cntl::SocResetReason;

const WIFI_ENABLED_MARKER: u8 = 0xA5;

#[esp_hal::ram(unstable(rtc_slow, persistent))]
static mut RETAINED_WIFI_ENABLED: u8 = 0;

static RESTORE_REQUEST: AtomicBool = AtomicBool::new(false);

/// Consume the retained request during boot. A marker is valid only when the
/// reset was the core reset emitted by ESP32-S3 deep sleep.
pub(crate) fn init(reset_reason: Option<SocResetReason>) {
    // SAFETY: startup consumes this byte before tasks are spawned. Volatile
    // access observes RTC contents retained across the previous reset.
    let marker = unsafe {
        let marker = core::ptr::read_volatile(core::ptr::addr_of!(RETAINED_WIFI_ENABLED));
        core::ptr::write_volatile(core::ptr::addr_of_mut!(RETAINED_WIFI_ENABLED), 0);
        marker
    };
    RESTORE_REQUEST.store(
        reset_reason == Some(SocResetReason::CoreDeepSleep) && marker == WIFI_ENABLED_MARKER,
        Ordering::Release,
    );
}

/// Return and clear the one-shot request after persisted credentials load.
pub(crate) fn take_restore_request() -> bool {
    RESTORE_REQUEST.swap(false, Ordering::AcqRel)
}

/// Arm the request at the final deep-sleep boundary.
pub(crate) fn arm_if_enabled(enabled: bool) {
    // SAFETY: only the UI sleep owner writes after peripheral quiescence,
    // immediately before deep sleep; startup is the only reader.
    unsafe {
        core::ptr::write_volatile(
            core::ptr::addr_of_mut!(RETAINED_WIFI_ENABLED),
            if enabled { WIFI_ENABLED_MARKER } else { 0 },
        );
    }
}
