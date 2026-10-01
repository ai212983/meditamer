#![forbid(unsafe_code)]

use super::*;
mod attempt;
mod config;
mod error;
mod events;
mod prepare;
mod recovery;
mod retry;
mod state_machine;
mod success;
mod task_state;
mod timeout;
mod timing;
use attempt::perform_connect_attempt;
use config::mode_config_from_credentials;
use error::handle_connect_error;
use events::{
    disconnect_reason_label, install_wifi_event_logger, is_auth_disconnect_reason,
    is_discovery_disconnect_reason, next_probe_channel, state_mem_stage,
};
use prepare::prepare_connection_attempt;
use recovery::{disconnect_and_stop_with_timeout, disconnect_with_timeout};
use state_machine::{apply_pending_runtime_policy_updates, transition_state};
use success::handle_connect_success;
use task_state::{ConnectionAttempt, WifiTaskState};
use timeout::handle_connect_timeout;

pub(super) use timing::{
    active_scan_timeout_ms, directed_scan_timeout_ms, passive_scan_timeout_ms,
    post_recover_watchdog_timeout_ms, zero_discovery_probe_timeout_ms,
};

pub(super) async fn run_wifi_connection_task(
    controller: &mut WifiController<'_>,
    _credentials: Option<WifiCredentials>,
    initial_policy: WifiRuntimePolicy,
    stack: Stack<'_>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let started_at = Instant::now();
    let mut state = WifiTaskState::new(_credentials, started_at);
    state.runtime_policy = initial_policy.sanitized();
    publish_config(state.credentials, state.runtime_policy);
    publish_state(
        state.net_state,
        state.ladder_step,
        state.net_attempt,
        state.failure_class,
        state.failure_code,
        state.started_at.elapsed().as_millis() as u32,
    );
    if state.credentials.is_none() {
        diag_wifi!("upload_http: waiting for NETCFG credentials over UART");
    }

    loop {
        acknowledge_control_quiescence().await;
        let active = match prepare_connection_attempt(controller, &mut state).await {
            ConnectionAttempt::Continue => continue,
            ConnectionAttempt::Proceed(active) => active,
        };
        perform_connect_attempt(controller, &stack, &mut state).await;
        state.credentials = Some(active);
    }
}

pub const fn boot_scan_only_diag_enabled() -> bool {
    false
}
