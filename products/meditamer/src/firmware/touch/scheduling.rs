use crate::firmware::types::i2c::{BusTiming, TouchWaits};
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};
use critical_section::Mutex;

static ACTIVE_SAMPLE_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVE_SAMPLE_GAP_MAX_MS: AtomicU32 = AtomicU32::new(0);
static LAST_ACTIVE_SAMPLE_MS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Default)]
pub(crate) struct RetryWakeTiming {
    pub armed_us: u32,
    pub irq_us: u32,
    pub resumed_us: u32,
    pub irq_events: u32,
    pub valid: bool,
}

impl RetryWakeTiming {
    const EMPTY: Self = Self {
        armed_us: 0,
        irq_us: 0,
        resumed_us: 0,
        irq_events: 0,
        valid: false,
    };
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TouchReadTiming {
    pub read_us: u32,
    pub bus: BusTiming,
    pub waits: TouchWaits,
    pub retry_wait_us: u32,
    pub retries: u8,
    pub retry_wakes: [RetryWakeTiming; 2],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TouchGapTiming {
    pub gap_ms: u32,
    pub previous_at_ms: u32,
    pub previous: TouchReadTiming,
    pub current: TouchReadTiming,
}

#[derive(Default)]
struct ReadState {
    previous: Option<(u32, TouchReadTiming)>,
    worst: TouchGapTiming,
}

static READ_STATE: Mutex<RefCell<ReadState>> = Mutex::new(RefCell::new(ReadState {
    previous: None,
    worst: TouchGapTiming {
        gap_ms: 0,
        previous_at_ms: 0,
        previous: TouchReadTiming {
            read_us: 0,
            bus: BusTiming {
                queue_us: 0,
                admission_us: 0,
                mutex_us: 0,
                config_us: 0,
                transfer_us: 0,
                attempts: 0,
                errors: 0,
            },
            waits: TouchWaits {
                attempts: [crate::firmware::types::i2c::TouchWait {
                    queue_us: 0,
                    busy_us: 0,
                    tail_us: 0,
                    last_two: 0,
                    transfers: 0,
                    holder: 0,
                }; 3],
                count: 0,
            },
            retry_wait_us: 0,
            retries: 0,
            retry_wakes: [RetryWakeTiming::EMPTY; 2],
        },
        current: TouchReadTiming {
            read_us: 0,
            bus: BusTiming {
                queue_us: 0,
                admission_us: 0,
                mutex_us: 0,
                config_us: 0,
                transfer_us: 0,
                attempts: 0,
                errors: 0,
            },
            waits: TouchWaits {
                attempts: [crate::firmware::types::i2c::TouchWait {
                    queue_us: 0,
                    busy_us: 0,
                    tail_us: 0,
                    last_two: 0,
                    transfers: 0,
                    holder: 0,
                }; 3],
                count: 0,
            },
            retry_wait_us: 0,
            retries: 0,
            retry_wakes: [RetryWakeTiming::EMPTY; 2],
        },
    },
}));

#[derive(Clone, Copy)]
pub(crate) struct TouchSchedulingSnapshot {
    pub(crate) active_sample_count: u32,
    pub(crate) active_sample_gap_max_ms: u32,
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
}

pub(crate) fn record_read(t_ms: u64, touch_count: u8, timing: TouchReadTiming) {
    critical_section::with(|cs| {
        let mut state = READ_STATE.borrow(cs).borrow_mut();
        if touch_count == 0 {
            state.previous = None;
            return;
        }
        let current = clamp_u32(t_ms);
        if let Some((previous_at_ms, previous)) = state.previous {
            let gap_ms = current.wrapping_sub(previous_at_ms);
            if gap_ms > state.worst.gap_ms {
                state.worst = TouchGapTiming {
                    gap_ms,
                    previous_at_ms,
                    previous,
                    current: timing,
                };
            }
        }
        state.previous = Some((current, timing));
    });
}

pub(crate) fn read_snapshot() -> TouchGapTiming {
    critical_section::with(|cs| READ_STATE.borrow(cs).borrow().worst)
}

/// End the current contact interval without discarding accumulated maxima.
/// Panel-driven suspension and fault recovery use the same boundary as the
/// qualification counter, so a later contact cannot create a false long gap.
pub(crate) fn pause() {
    LAST_ACTIVE_SAMPLE_MS.store(0, Ordering::Relaxed);
    critical_section::with(|cs| READ_STATE.borrow(cs).borrow_mut().previous = None);
}

pub(crate) fn snapshot() -> TouchSchedulingSnapshot {
    TouchSchedulingSnapshot {
        active_sample_count: ACTIVE_SAMPLE_COUNT.load(Ordering::Relaxed),
        active_sample_gap_max_ms: ACTIVE_SAMPLE_GAP_MAX_MS.load(Ordering::Relaxed),
    }
}

pub(crate) fn reset() {
    ACTIVE_SAMPLE_COUNT.store(0, Ordering::Relaxed);
    ACTIVE_SAMPLE_GAP_MAX_MS.store(0, Ordering::Relaxed);
    LAST_ACTIVE_SAMPLE_MS.store(0, Ordering::Relaxed);
    critical_section::with(|cs| *READ_STATE.borrow(cs).borrow_mut() = ReadState::default());
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
