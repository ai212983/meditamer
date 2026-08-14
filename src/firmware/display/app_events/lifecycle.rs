use crate::firmware::types::{DisplayContext, TouchStatus};

use super::super::state::DisplayLoopState;

pub(super) async fn handle_battery_tick_event(context: &mut DisplayContext, upload_enabled: bool) {
    if upload_enabled {
        return;
    }
    if let Ok(sampled_percent) = context.inkplate.fuel_gauge_soc().await {
        if sampled_percent <= 100 {
            let sampled_percent = sampled_percent as u8;
            crate::firmware::imu::publish_trace_context(crate::firmware::imu::ImuTraceContext {
                battery_percent: i16::from(sampled_percent),
            });
        }
    }
}

pub(super) async fn handle_touch_status_event(
    status: TouchStatus,
    _context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    state.touch_startup_settled = !matches!(status, TouchStatus::Initializing);
}
