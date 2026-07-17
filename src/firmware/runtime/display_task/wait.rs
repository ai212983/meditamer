use embassy_time::Instant;

use crate::firmware::config::UI_TICK_MS;

use super::state::DisplayLoopState;

pub(super) fn next_loop_wait_ms(state: &DisplayLoopState) -> u64 {
    let now = Instant::now();
    let mut wait_ms = UI_TICK_MS;

    if state.touch_feedback_dirty {
        wait_ms = wait_ms.min(ms_until(now, state.touch_feedback_next_flush_at));
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
