async fn handle_touch_status_event(
    status: TouchStatus,
    _context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    state.touch_ready = matches!(status, TouchStatus::Ready { .. });
    if !matches!(status, TouchStatus::Initializing) {
        state.touch_startup_settled = true;
    }
}

async fn handle_start_touch_calibration_wizard_event(
    _context: &mut DisplayContext,
    _state: &mut DisplayLoopState,
    _upload_enabled: bool,
) {
}

async fn setup_touch_wizard_screen(_context: &mut DisplayContext, _state: &mut DisplayLoopState) {}
