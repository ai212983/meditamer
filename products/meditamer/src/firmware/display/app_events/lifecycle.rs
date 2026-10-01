use crate::firmware::types::{DisplayContext, TouchStatus};

use super::super::state::DisplayLoopState;

pub(super) async fn handle_touch_status_event(
    status: TouchStatus,
    _context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    state.touch_startup_settled = !matches!(status, TouchStatus::Initializing);
}
