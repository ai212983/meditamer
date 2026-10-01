use crate::firmware::types::DisplayContext;

use super::super::state::DisplayLoopState;

pub(super) async fn handle_force_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    request_id: Option<u64>,
) {
    match request_id {
        Some(id) => {
            super::super::panel::refresh::force_full_repaint_traced(
                context,
                &mut state.presentation,
                "serial_repaint",
                Some(id),
            )
            .await;
        }
        None => {
            super::super::presentation::force_full_repaint(
                context,
                &mut state.presentation,
                "serial_repaint",
            )
            .await;
        }
    }
}
