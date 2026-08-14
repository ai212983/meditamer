use core::sync::atomic::{AtomicU32, Ordering};

static ACTIVE_SAMPLE_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE_SAMPLE_GAP_MAX_MS: AtomicU32 = AtomicU32::new(0);
static ACTIVE_SAMPLE_GAP_OVER_16_MS: AtomicU32 = AtomicU32::new(0);
static LAST_ACTIVE_SAMPLE_MS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
pub(crate) struct TouchSchedulingSnapshot {
    pub(crate) active_sample_count: u32,
    pub(crate) active_sample_gap_max_ms: u32,
    pub(crate) active_sample_gap_over_16_ms: u32,
}

pub(crate) fn record_sample(t_ms: u64, touch_count: u8) {
    if touch_count == 0 {
        LAST_ACTIVE_SAMPLE_MS.store(0, Ordering::Relaxed);
        return;
    }

    ACTIVE_SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
    let current = clamp_u32(t_ms);
    let previous = LAST_ACTIVE_SAMPLE_MS.swap(current, Ordering::Relaxed);
    if previous == 0 {
        return;
    }
    let gap_ms = current.wrapping_sub(previous);
    update_max(&ACTIVE_SAMPLE_GAP_MAX_MS, gap_ms);
    if gap_ms > 16 {
        ACTIVE_SAMPLE_GAP_OVER_16_MS.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn snapshot() -> TouchSchedulingSnapshot {
    TouchSchedulingSnapshot {
        active_sample_count: ACTIVE_SAMPLE_COUNT.load(Ordering::Relaxed),
        active_sample_gap_max_ms: ACTIVE_SAMPLE_GAP_MAX_MS.load(Ordering::Relaxed),
        active_sample_gap_over_16_ms: ACTIVE_SAMPLE_GAP_OVER_16_MS.load(Ordering::Relaxed),
    }
}

pub(crate) fn reset() {
    ACTIVE_SAMPLE_COUNT.store(0, Ordering::Relaxed);
    ACTIVE_SAMPLE_GAP_MAX_MS.store(0, Ordering::Relaxed);
    ACTIVE_SAMPLE_GAP_OVER_16_MS.store(0, Ordering::Relaxed);
    LAST_ACTIVE_SAMPLE_MS.store(0, Ordering::Relaxed);
}

fn clamp_u32(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

fn update_max(counter: &AtomicU32, value: u32) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}
