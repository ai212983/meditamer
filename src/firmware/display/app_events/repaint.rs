use crate::firmware::types::DisplayContext;

use super::super::state::DisplayLoopState;

pub(super) async fn handle_force_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    super::super::presentation::force_full_repaint(
        context,
        &mut state.presentation,
        "serial_repaint",
    )
    .await;
}
