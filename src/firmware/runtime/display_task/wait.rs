use embassy_time::Instant;

use crate::firmware::config::{TAP_TRACE_ENABLED, UI_TICK_MS};

use super::state::DisplayLoopState;

pub(super) fn next_loop_wait_ms(state: &DisplayLoopState) -> u64 {
    let now = Instant::now();
    let mut wait_ms = UI_TICK_MS;

    if state.touch_ready {
        wait_ms = wait_ms.min(ms_until(now, state.touch_next_sample_at));
    } else {
        wait_ms = wait_ms.min(ms_until(now, state.touch_retry_at));
    }

    if !state.imu_double_tap_ready {
        wait_ms = wait_ms.min(ms_until(now, state.imu_retry_at));
    }

    if state.touch_feedback_dirty {
        wait_ms = wait_ms.min(ms_until(now, state.touch_feedback_next_flush_at));
    }

    if TAP_TRACE_ENABLED && state.imu_double_tap_ready {
        wait_ms = wait_ms.min(ms_until(now, state.tap_trace_next_sample_at));
        wait_ms = wait_ms.min(ms_until(now, state.tap_trace_aux_next_sample_at));
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
