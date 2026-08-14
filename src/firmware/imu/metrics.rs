use core::sync::atomic::{AtomicU32, Ordering};

use super::{scheduler::SamplingMode, types::ImuSuppressionReason};

static IDLE_SAMPLES: AtomicU32 = AtomicU32::new(0);
static ACTIVE_SAMPLES: AtomicU32 = AtomicU32::new(0);
static SAMPLE_GAP_MAX_MS: AtomicU32 = AtomicU32::new(0);
static LAST_SAMPLE_MS: AtomicU32 = AtomicU32::new(0);
static PROMOTIONS: AtomicU32 = AtomicU32::new(0);
static DEMOTIONS: AtomicU32 = AtomicU32::new(0);
static MISSED_DEADLINES: AtomicU32 = AtomicU32::new(0);
static TOUCH_SUPPRESSED: AtomicU32 = AtomicU32::new(0);
static UPLOAD_SUPPRESSED: AtomicU32 = AtomicU32::new(0);
static DISCONTINUITIES: AtomicU32 = AtomicU32::new(0);
static INIT_FAILURES: AtomicU32 = AtomicU32::new(0);
static SAMPLE_FAILURES: AtomicU32 = AtomicU32::new(0);
static RECOVERIES: AtomicU32 = AtomicU32::new(0);
static ACTION_COALESCED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
pub(crate) struct ImuMetricsSnapshot {
    pub(crate) idle_samples: u32,
    pub(crate) active_samples: u32,
    pub(crate) sample_gap_max_ms: u32,
    pub(crate) promotions: u32,
    pub(crate) demotions: u32,
    pub(crate) missed_deadlines: u32,
    pub(crate) touch_suppressed: u32,
    pub(crate) upload_suppressed: u32,
    pub(crate) discontinuities: u32,
    pub(crate) init_failures: u32,
    pub(crate) sample_failures: u32,
    pub(crate) recoveries: u32,
    pub(crate) action_coalesced: u32,
}

pub(crate) fn record_sample(now_ms: u64, mode: SamplingMode) {
    match mode {
        SamplingMode::Idle => IDLE_SAMPLES.fetch_add(1, Ordering::Relaxed),
        SamplingMode::Active => ACTIVE_SAMPLES.fetch_add(1, Ordering::Relaxed),
    };
    let current = clamp_u32(now_ms);
    let previous = LAST_SAMPLE_MS.swap(current, Ordering::Relaxed);
    if previous != 0 {
        update_max(&SAMPLE_GAP_MAX_MS, current.wrapping_sub(previous));
    }
}

pub(crate) fn record_mode_change(from: SamplingMode, to: SamplingMode) {
    match (from, to) {
        (SamplingMode::Idle, SamplingMode::Active) => {
            PROMOTIONS.fetch_add(1, Ordering::Relaxed);
        }
        (SamplingMode::Active, SamplingMode::Idle) => {
            DEMOTIONS.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

pub(crate) fn record_missed_deadline() {
    MISSED_DEADLINES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_suppressed(reason: ImuSuppressionReason) {
    match reason {
        ImuSuppressionReason::Touch => TOUCH_SUPPRESSED.fetch_add(1, Ordering::Relaxed),
        ImuSuppressionReason::Upload => UPLOAD_SUPPRESSED.fetch_add(1, Ordering::Relaxed),
    };
}

pub(crate) fn record_discontinuity() {
    DISCONTINUITIES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_init_failure() {
    INIT_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_sample_failure() {
    SAMPLE_FAILURES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_recovery() {
    RECOVERIES.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_action_coalesced() {
    ACTION_COALESCED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> ImuMetricsSnapshot {
    ImuMetricsSnapshot {
        idle_samples: IDLE_SAMPLES.load(Ordering::Relaxed),
        active_samples: ACTIVE_SAMPLES.load(Ordering::Relaxed),
        sample_gap_max_ms: SAMPLE_GAP_MAX_MS.load(Ordering::Relaxed),
        promotions: PROMOTIONS.load(Ordering::Relaxed),
        demotions: DEMOTIONS.load(Ordering::Relaxed),
        missed_deadlines: MISSED_DEADLINES.load(Ordering::Relaxed),
        touch_suppressed: TOUCH_SUPPRESSED.load(Ordering::Relaxed),
        upload_suppressed: UPLOAD_SUPPRESSED.load(Ordering::Relaxed),
        discontinuities: DISCONTINUITIES.load(Ordering::Relaxed),
        init_failures: INIT_FAILURES.load(Ordering::Relaxed),
        sample_failures: SAMPLE_FAILURES.load(Ordering::Relaxed),
        recoveries: RECOVERIES.load(Ordering::Relaxed),
        action_coalesced: ACTION_COALESCED.load(Ordering::Relaxed),
    }
}

pub(crate) fn reset() {
    for counter in [
        &IDLE_SAMPLES,
        &ACTIVE_SAMPLES,
        &SAMPLE_GAP_MAX_MS,
        &LAST_SAMPLE_MS,
        &PROMOTIONS,
        &DEMOTIONS,
        &MISSED_DEADLINES,
        &TOUCH_SUPPRESSED,
        &UPLOAD_SUPPRESSED,
        &DISCONTINUITIES,
        &INIT_FAILURES,
        &SAMPLE_FAILURES,
        &RECOVERIES,
        &ACTION_COALESCED,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
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
