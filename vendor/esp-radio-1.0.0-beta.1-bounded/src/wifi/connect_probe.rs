//! Temporary reset-retained phase marker for an unresponsive Wi-Fi connect.
//!
//! The marker has no UART, heap, or lock use. The task watchdog can observe
//! it from the other core, and boot prints the last phase after a watchdog
//! reset. Dropping the async future marks it complete, including timeouts.

use portable_atomic::{AtomicU32, Ordering};

const MAGIC_VALUE: u32 = 0x4350_4831; // CPH1
/// Event subscription acquired; synchronous connect has not started.
pub const SUBSCRIBED: u32 = 2;
/// The synchronous driver connect call has been entered.
pub const DRIVER_ENTER: u32 = 3;
/// The synchronous driver connect call returned.
pub const DRIVER_RETURN: u32 = 4;
/// The async task is waiting for a connected/disconnected event.
pub const EVENT_WAIT: u32 = 5;
/// A terminal connection event was received.
pub const EVENT_RECEIVED: u32 = 6;
/// The connect future returned or was cancelled.
pub const DONE: u32 = 7;

// RTC slow memory is retained across the timer-group watchdog reset used by
// the incident capture. A host-controlled DTR/RTS power-on reset need not
// preserve it; do not interpret an absent record after that reset as healthy.
#[unsafe(link_section = ".rtc_slow.persistent")]
static ATTEMPT: AtomicU32 = AtomicU32::new(0);
#[unsafe(link_section = ".rtc_slow.persistent")]
static STATE: AtomicU32 = AtomicU32::new(0);
#[unsafe(link_section = ".rtc_slow.persistent")]
static MAGIC: AtomicU32 = AtomicU32::new(0);

/// Marks the attempt complete even when a timeout drops the future.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        mark(DONE);
    }
}

/// Begin a new attempt and commit its first phase to RTC slow memory.
pub fn begin() -> Guard {
    let attempt = ATTEMPT.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    STATE.store((attempt << 8) | 1, Ordering::Release);
    MAGIC.store(MAGIC_VALUE, Ordering::Release);
    Guard
}

/// Publish a connect phase without allocation, logging, or locking.
pub fn mark(phase: u32) {
    let attempt = ATTEMPT.load(Ordering::Relaxed);
    STATE.store((attempt << 8) | phase, Ordering::Release);
}

/// A single atomic state word prevents cross-core phase/attempt tearing.
pub fn current() -> u32 {
    if MAGIC.load(Ordering::Acquire) == MAGIC_VALUE {
        STATE.load(Ordering::Acquire)
    } else {
        0
    }
}

/// Return the previous attempt's last phase and clear the boot marker.
pub fn take() -> u32 {
    let state = current();
    MAGIC.store(0, Ordering::Release);
    STATE.store(0, Ordering::Release);
    state
}
