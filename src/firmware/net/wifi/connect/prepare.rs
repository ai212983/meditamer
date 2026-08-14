use super::*;

mod preconditions;
mod scan;
mod start;

use preconditions::{prepare_preconditions, PreparePreconditions};
use scan::prepare_scan_candidates;
use start::prepare_budget_and_start;

pub(super) async fn prepare_connection_attempt(
    controller: &mut WifiController<'_>,
    state: &mut WifiTaskState,
) -> ConnectionAttempt {
    let active = match prepare_preconditions(controller, state).await {
        PreparePreconditions::Continue => return ConnectionAttempt::Continue,
        PreparePreconditions::Active(active) => active,
    };

    if prepare_budget_and_start(controller, state, active).await {
        return ConnectionAttempt::Continue;
    }

    if prepare_scan_candidates(controller, state, active).await {
        return ConnectionAttempt::Continue;
    }

    WIFI_DISCONNECTED_EVENT.store(false, Ordering::Relaxed);
    state.net_attempt = state.net_attempt.saturating_add(1);
    state.ladder_step = RecoveryLadderStep::RetrySame;
    transition_state(
        &mut state.net_state,
        NetState::Associating,
        "connect_begin",
        state.started_at,
        state.ladder_step,
        state.net_attempt,
        (state.failure_class, state.failure_code),
    );
    publish_state(
        state.net_state,
        state.ladder_step,
        state.net_attempt,
        state.failure_class,
        state.failure_code,
        state.started_at.elapsed().as_millis() as u32,
    );
    state.note_hard_recover_connect_begin();
    observability::record_wifi_reassoc_connect_begin(
        state.auth_method_idx,
        state.channel_hint,
        state.channel_probe_idx,
    );
    observability::record_wifi_connect_attempt(state.channel_hint, state.auth_method_idx);

    ConnectionAttempt::Proceed(active)
}
