use super::*;
mod attempt;
mod config;
mod driver_state;
mod error;
mod events;
mod idf_log_diag;
mod idf_scan_compare;
mod prepare;
mod promisc_diag;
mod recovery;
mod retry;
mod state_machine;
mod success;
mod task_state;
mod timeout;
mod timing;
use attempt::perform_connect_attempt;
use config::mode_config_from_credentials;
use driver_state::{
    maybe_log_first_start_driver_state, maybe_log_pre_start_driver_state,
    maybe_log_scan_entry_driver_state,
};
use error::handle_connect_error;
use events::{
    disconnect_reason_label, install_wifi_event_logger, is_auth_disconnect_reason,
    is_discovery_disconnect_reason, next_probe_channel, state_mem_stage,
};
use idf_log_diag::maybe_begin_first_start_idf_log_diag;
use idf_scan_compare::maybe_run_scan_entry_idf_compare_diag;
use prepare::prepare_connection_attempt;
use promisc_diag::{maybe_handle_post_start_promisc_diag, maybe_run_scan_entry_promisc_diag};
use recovery::{
    disconnect_and_stop_with_timeout, disconnect_with_timeout,
    maybe_software_reset_on_zero_discovery_hard_guard,
    maybe_software_reset_on_zero_discovery_terminal,
};
use state_machine::{apply_pending_runtime_policy_updates, transition_state};
use success::handle_connect_success;
use task_state::{ConnectionAttempt, WifiTaskState};
use timeout::handle_connect_timeout;

pub(super) use config::{wifi_credentials, wifi_credentials_from_parts};
pub(super) use timing::{
    active_scan_timeout_ms, directed_scan_timeout_ms, passive_scan_timeout_ms,
    post_recover_watchdog_timeout_ms, zero_discovery_probe_timeout_ms,
};

pub(super) async fn run_wifi_connection_task(
    mut controller: WifiController<'static>,
    _credentials: Option<WifiCredentials>,
    stack: Stack<'static>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let started_at = Instant::now();
    let mut state = WifiTaskState::new(_credentials, started_at);
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
        let active = match prepare_connection_attempt(&mut controller, &mut state).await {
            ConnectionAttempt::Continue => continue,
            ConnectionAttempt::Proceed(active) => active,
        };
        perform_connect_attempt(&mut controller, &stack, &mut state).await;
        state.credentials = Some(active);
    }
}

pub(crate) const fn boot_scan_only_diag_enabled() -> bool {
    false
}

pub(super) fn monotonic_now_ms_u32() -> u32 {
    Instant::now().as_millis() as u32
}

pub(super) fn tick_age_ms_u32(last_tick_ms: u32) -> i64 {
    if last_tick_ms == 0 {
        return -1;
    }
    let now = monotonic_now_ms_u32();
    i64::from(now.wrapping_sub(last_tick_ms))
}

const WIFI_REAPPLY_PROTOCOL_AFTER_START: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_REAPPLY_PROTOCOL_AFTER_START") {
        Some(value) => Some(value),
        None => option_env!("WIFI_REAPPLY_PROTOCOL_AFTER_START"),
    },
);
const WIFI_C_LIKE_DISCOVERY_START: bool =
    parse_nonzero_flag(match option_env!("MEDITAMER_WIFI_C_LIKE_DISCOVERY_START") {
        Some(value) => Some(value),
        None => option_env!("WIFI_C_LIKE_DISCOVERY_START"),
    });

pub(super) fn maybe_reapply_sta_protocol_after_start(controller: &mut WifiController<'static>) {
    if !WIFI_REAPPLY_PROTOCOL_AFTER_START {
        return;
    }
    let protocols = wifi_standard_bgn_protocols();
    match wifi_set_protocol(controller, protocols) {
        Ok(()) => diag_reassoc!("upload_http: post_start_protocol_reapply result=ok profile=bgn"),
        Err(err) => diag_reassoc!(
            "upload_http: post_start_protocol_reapply result=err profile=bgn err={:?}",
            err,
        ),
    }
}
