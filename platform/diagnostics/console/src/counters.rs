//! Cumulative UART drop causes. Each failed log adds one to the total and to
//! exactly one cause. Counters only increase; there is no clearing.
use core::sync::atomic::{AtomicU32, Ordering};

static DROPPED_TOTAL: AtomicU32 = AtomicU32::new(0);
static DROPPED_CONTENTION: AtomicU32 = AtomicU32::new(0);
static DROPPED_DEFERRED_OVERFLOW: AtomicU32 = AtomicU32::new(0);
static DROPPED_DEFERRED_OVERSIZE: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropCounts {
    pub total: u32,
    pub contention: u32,
    pub deferred_overflow: u32,
    pub deferred_oversize: u32,
    pub stable: bool,
}

pub fn dropped_write_count() -> u32 {
    DROPPED_TOTAL.load(Ordering::Relaxed)
}

pub fn drop_counts() -> DropCounts {
    const ATTEMPTS: u32 = 8;
    let mut last = (0u32, 0u32, 0u32, 0u32);
    for _ in 0..ATTEMPTS {
        let first = DROPPED_TOTAL.load(Ordering::Relaxed);
        let contention = DROPPED_CONTENTION.load(Ordering::Relaxed);
        let overflow = DROPPED_DEFERRED_OVERFLOW.load(Ordering::Relaxed);
        let oversize = DROPPED_DEFERRED_OVERSIZE.load(Ordering::Relaxed);
        let second = DROPPED_TOTAL.load(Ordering::Relaxed);
        last = (second, contention, overflow, oversize);
        // A concurrent drop separates the two total loads, or breaks the
        // wrapping sum when its two increments straddle these loads.
        if first == second && second == contention.wrapping_add(overflow).wrapping_add(oversize) {
            return DropCounts {
                total: second,
                contention,
                deferred_overflow: overflow,
                deferred_oversize: oversize,
                stable: true,
            };
        }
    }
    DropCounts {
        total: last.0,
        contention: last.1,
        deferred_overflow: last.2,
        deferred_oversize: last.3,
        stable: false,
    }
}

// The cause increment precedes the total increment; a torn snapshot fails the
// wrapping-sum check above and retries instead of reporting stable.
pub fn record_contention() {
    DROPPED_CONTENTION.fetch_add(1, Ordering::Relaxed);
    DROPPED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[cfg(feature = "deferred-logs")]
pub fn record_deferred_overflow() {
    DROPPED_DEFERRED_OVERFLOW.fetch_add(1, Ordering::Relaxed);
    DROPPED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

#[cfg(feature = "deferred-logs")]
pub fn record_deferred_oversize() {
    DROPPED_DEFERRED_OVERSIZE.fetch_add(1, Ordering::Relaxed);
    DROPPED_TOTAL.fetch_add(1, Ordering::Relaxed);
}
