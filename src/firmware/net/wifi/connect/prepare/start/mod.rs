use super::*;

mod config;
mod sequence;

use config::{ensure_station_config_applied, should_use_c_like_discovery_start};
use sequence::{handle_status_err, run_start_driver};

pub(super) async fn prepare_budget_and_start(
    controller: &mut WifiController<'static>,
    state: &mut WifiTaskState,
    active: WifiCredentials,
) -> bool {
    if state.net_attempt >= policy_total_attempt_budget(state.runtime_policy) {
        state.terminal_fail_latched = true;
        state.ladder_step = RecoveryLadderStep::TerminalFail;
        if matches!(state.failure_class, NetFailureClass::None) {
            state.failure_class = NetFailureClass::Unknown;
        }
        if state.failure_code == 0 {
            state.failure_code = WIFI_REASON_OTHER;
        }
        transition_state(
            &mut state.net_state,
            NetState::Failed,
            "attempt_budget_exhausted",
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
        Timer::after(Duration::from_millis(
            state.runtime_policy.cooldown_ms.max(250) as u64,
        ))
        .await;
        return true;
    }

    let c_like_discovery_start = should_use_c_like_discovery_start(state);

    if !state.config_applied
        && !c_like_discovery_start
        && ensure_station_config_applied(controller, state, active).await
    {
        return true;
    }

    match wifi_is_started(controller) {
        Ok(true) => {}
        Ok(false) => {
            if run_start_driver(controller, state, c_like_discovery_start).await {
                return true;
            }
        }
        Err(err) => {
            return handle_status_err(controller, state, err).await;
        }
    }

    if !state.config_applied && ensure_station_config_applied(controller, state, active).await {
        return true;
    }
    if state.escalated_auth_sweep_attempts_left > 0 {
        state.channel_hint = None;
        state.bssid_hint = None;
        state.ap_candidates.clear();
        state.ap_candidate_idx = 0;
        state.channel_probe_idx = 0;
    }

    false
}
