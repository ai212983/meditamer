use embassy_time::Instant;

use crate::firmware::config::UI_TICK_MS;

pub(super) struct LoopWaitSchedule {
    pub(super) touch_ready: bool,
    pub(super) touch_retry_at: Instant,
    pub(super) touch_next_sample_at: Instant,
    pub(super) imu_ready: bool,
    pub(super) imu_retry_at: Instant,
    pub(super) touch_feedback_dirty: bool,
    pub(super) touch_feedback_next_flush_at: Instant,
    pub(super) tap_trace_active: bool,
    pub(super) tap_trace_next_sample_at: Instant,
    pub(super) tap_trace_aux_next_sample_at: Instant,
}

pub(super) fn next_loop_wait_ms(schedule: LoopWaitSchedule) -> u64 {
    let now = Instant::now();
    let mut wait_ms = UI_TICK_MS;

    if schedule.touch_ready {
        wait_ms = wait_ms.min(ms_until(now, schedule.touch_next_sample_at));
    } else {
        wait_ms = wait_ms.min(ms_until(now, schedule.touch_retry_at));
    }

    if !schedule.imu_ready {
        wait_ms = wait_ms.min(ms_until(now, schedule.imu_retry_at));
    }

    if schedule.touch_feedback_dirty {
        wait_ms = wait_ms.min(ms_until(now, schedule.touch_feedback_next_flush_at));
    }

    if schedule.tap_trace_active {
        wait_ms = wait_ms.min(ms_until(now, schedule.tap_trace_next_sample_at));
        wait_ms = wait_ms.min(ms_until(now, schedule.tap_trace_aux_next_sample_at));
    }

    wait_ms
}

fn ms_until(now: Instant, deadline: Instant) -> u64 {
    if deadline <= now {
        0
    } else {
        deadline.saturating_duration_since(now).as_millis()
    }
}
