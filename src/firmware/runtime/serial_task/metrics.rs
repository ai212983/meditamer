use core::{fmt::Write, sync::atomic::Ordering};

use crate::firmware::{
    config::{LAST_MARBLE_REDRAW_MS, MAX_MARBLE_REDRAW_MS},
    telemetry,
    touch::debug_log::uart_write_all,
    types::SerialUart,
};

use super::commands::{TelemetryDomain, TelemetrySetOperation};

pub(super) async fn write_metrics_lines(uart: &mut SerialUart) {
    let last_ms = LAST_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
    let max_ms = MAX_MARBLE_REDRAW_MS.load(Ordering::Relaxed);
    let snapshot = telemetry::snapshot();

    let mut line = heapless::String::<160>::new();
    let _ = write!(
        &mut line,
        "METRICS MARBLE_REDRAW_MS={} MAX_MS={}\r\n",
        last_ms, max_ms
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;

    let mut wifi_line = heapless::String::<256>::new();
    let _ = write!(
        &mut wifi_line,
        "METRICS WIFI attempt={} success={} failure={} no_ap={} scan_runs={} scan_empty={} scan_hits={}\r\n",
        snapshot.wifi_connect_attempts,
        snapshot.wifi_connect_successes,
        snapshot.wifi_connect_failures,
        snapshot.wifi_reason_no_ap_found,
        snapshot.wifi_scan_runs,
        snapshot.wifi_scan_empty,
        snapshot.wifi_scan_target_hits,
    );
    let _ = uart_write_all(uart, wifi_line.as_bytes()).await;

    let mut wifi_link_line = heapless::String::<192>::new();
    let _ = write!(
        &mut wifi_link_line,
        "METRICS WIFI_LINK rssi_last_dbm={} rssi_min_dbm={} rssi_max_dbm={} rssi_samples={} rssi_low_samples={}\r\n",
        snapshot.wifi_link_rssi_last_dbm,
        snapshot.wifi_link_rssi_min_dbm,
        snapshot.wifi_link_rssi_max_dbm,
        snapshot.wifi_link_rssi_samples,
        snapshot.wifi_link_rssi_low_samples,
    );
    let _ = uart_write_all(uart, wifi_link_line.as_bytes()).await;

    let mut wifi_reassoc_line = heapless::String::<512>::new();
    let _ = write!(
        &mut wifi_reassoc_line,
        "METRICS WIFI_REASSOC mode_pause={} mode_resume={} cred_rx={} cred_chg={} cfg_apply={} start_ok={} start_err={} conn_begin={} conn_ok={} conn_err={} disc_evt={} probe={} auth_rot={} hint_retry={} conn_ms={} conn_ms_max={}\r\n",
        snapshot.wifi_reassoc_mode_pauses,
        snapshot.wifi_reassoc_mode_resumes,
        snapshot.wifi_reassoc_credentials_received,
        snapshot.wifi_reassoc_credentials_changed,
        snapshot.wifi_reassoc_config_applied,
        snapshot.wifi_reassoc_start_ok,
        snapshot.wifi_reassoc_start_err,
        snapshot.wifi_reassoc_connect_begin,
        snapshot.wifi_reassoc_connect_success,
        snapshot.wifi_reassoc_connect_failure,
        snapshot.wifi_reassoc_disconnect_events,
        snapshot.wifi_reassoc_channel_probes,
        snapshot.wifi_reassoc_auth_rotations,
        snapshot.wifi_reassoc_hint_retries,
        snapshot.wifi_reassoc_connect_ms_total,
        snapshot.wifi_reassoc_connect_ms_max,
    );
    let _ = uart_write_all(uart, wifi_reassoc_line.as_bytes()).await;

    let mut wifi_scan_diag_line = heapless::String::<512>::new();
    let _ = write!(
        &mut wifi_scan_diag_line,
        "METRICS WIFI_SCAN_DIAG active_n={} active_empty={} active_hit={} active_ms={} active_ms_max={} passive_n={} passive_empty={} passive_hit={} passive_ms={} passive_ms_max={} last_scan_ch={}\r\n",
        snapshot.wifi_reassoc_scan_active_runs,
        snapshot.wifi_reassoc_scan_active_empty,
        snapshot.wifi_reassoc_scan_active_hits,
        snapshot.wifi_reassoc_scan_active_ms_total,
        snapshot.wifi_reassoc_scan_active_ms_max,
        snapshot.wifi_reassoc_scan_passive_runs,
        snapshot.wifi_reassoc_scan_passive_empty,
        snapshot.wifi_reassoc_scan_passive_hits,
        snapshot.wifi_reassoc_scan_passive_ms_total,
        snapshot.wifi_reassoc_scan_passive_ms_max,
        snapshot.wifi_reassoc_last_scan_channel,
    );
    let _ = uart_write_all(uart, wifi_scan_diag_line.as_bytes()).await;

    let mut wifi_reason_diag_line = heapless::String::<512>::new();
    let _ = write!(
        &mut wifi_reason_diag_line,
        "METRICS WIFI_REASON_DIAG r2={} r201={} r202={} r203={} r204={} r205={} r210={} r211={} r212={} rother={} last_reason={} last_auth={} last_ch={} last_probe={} last_stage={}\r\n",
        snapshot.wifi_reassoc_reason_2,
        snapshot.wifi_reassoc_reason_201,
        snapshot.wifi_reassoc_reason_202,
        snapshot.wifi_reassoc_reason_203,
        snapshot.wifi_reassoc_reason_204,
        snapshot.wifi_reassoc_reason_205,
        snapshot.wifi_reassoc_reason_210,
        snapshot.wifi_reassoc_reason_211,
        snapshot.wifi_reassoc_reason_212,
        snapshot.wifi_reassoc_reason_other,
        snapshot.wifi_reassoc_last_reason,
        snapshot.wifi_reassoc_last_auth_idx,
        snapshot.wifi_reassoc_last_channel_hint,
        snapshot.wifi_reassoc_last_probe_idx,
        snapshot.wifi_reassoc_last_stage,
    );
    let _ = uart_write_all(uart, wifi_reason_diag_line.as_bytes()).await;

    let mut upload_line = heapless::String::<384>::new();
    let _ = write!(
        &mut upload_line,
        "METRICS UPLOAD accept_ok={} accept_err={} request_err={} req_hdr_to={} req_read_body={} req_read_body_reset={} req_sd_busy={} sd_errors={} sd_busy={} sd_timeouts={} sd_power_on_fail={} sd_init_fail={} sess_timeout_abort={} sess_mode_off_abort={}\r\n",
        snapshot.upload_http_accepts,
        snapshot.upload_http_accept_errors,
        snapshot.upload_http_request_errors,
        snapshot.upload_http_header_timeouts,
        snapshot.upload_http_read_body_errors,
        snapshot.upload_http_read_body_resets,
        snapshot.upload_http_sd_busy_errors,
        snapshot.sd_upload_errors,
        snapshot.sd_upload_busy,
        snapshot.sd_upload_timeouts,
        snapshot.sd_upload_power_on_failed,
        snapshot.sd_upload_init_failed,
        snapshot.sd_upload_session_timeout_aborts,
        snapshot.sd_upload_session_mode_off_aborts,
    );
    let _ = uart_write_all(uart, upload_line.as_bytes()).await;

    let mut upload_phase_line = heapless::String::<320>::new();
    let _ = write!(
        &mut upload_phase_line,
        "METRICS UPLOAD_PHASE req={} bytes={} body_ms={} body_max={} sd_ms={} sd_max={} req_ms={} req_max={}\r\n",
        snapshot.upload_http_upload_requests,
        snapshot.upload_http_upload_bytes,
        snapshot.upload_http_upload_body_read_ms_total,
        snapshot.upload_http_upload_body_read_ms_max,
        snapshot.upload_http_upload_sd_wait_ms_total,
        snapshot.upload_http_upload_sd_wait_ms_max,
        snapshot.upload_http_upload_request_ms_total,
        snapshot.upload_http_upload_request_ms_max,
    );
    let _ = uart_write_all(uart, upload_phase_line.as_bytes()).await;

    let mut upload_decomp_line = heapless::String::<512>::new();
    let _ = write!(
        &mut upload_decomp_line,
        "METRICS UPLOAD_DECOMP copy_ms={} copy_max={} sdq_ms={} sdq_max={} sdtask_ms={} sdtask_max={} commit_ms={} commit_max={} chunk_p50_max={} chunk_p95_max={} chunk_max={} chunk_samples={} chunk_drop={}\r\n",
        snapshot.upload_http_upload_payload_copy_ms_total,
        snapshot.upload_http_upload_payload_copy_ms_max,
        snapshot.upload_http_upload_sd_queue_ms_total,
        snapshot.upload_http_upload_sd_queue_ms_max,
        snapshot.upload_http_upload_sd_task_wait_ms_total,
        snapshot.upload_http_upload_sd_task_wait_ms_max,
        snapshot.upload_http_upload_commit_ms_total,
        snapshot.upload_http_upload_commit_ms_max,
        snapshot.upload_http_upload_chunk_p50_ms_max,
        snapshot.upload_http_upload_chunk_p95_ms_max,
        snapshot.upload_http_upload_chunk_max_ms_max,
        snapshot.upload_http_upload_chunk_samples_total,
        snapshot.upload_http_upload_chunk_samples_dropped,
    );
    let _ = uart_write_all(uart, upload_decomp_line.as_bytes()).await;

    let mut upload_rtt_line = heapless::String::<512>::new();
    let _ = write!(
        &mut upload_rtt_line,
        "METRICS UPLOAD_RTT begin_n={} begin_ms={} begin_max={} chunk_n={} chunk_ms={} chunk_max={} commit_n={} commit_ms={} commit_max={} abort_n={} abort_ms={} abort_max={} mkdir_n={} mkdir_ms={} mkdir_max={} rm_n={} rm_ms={} rm_max={}\r\n",
        snapshot.sd_upload_rtt_begin_count,
        snapshot.sd_upload_rtt_begin_ms_total,
        snapshot.sd_upload_rtt_begin_ms_max,
        snapshot.sd_upload_rtt_chunk_count,
        snapshot.sd_upload_rtt_chunk_ms_total,
        snapshot.sd_upload_rtt_chunk_ms_max,
        snapshot.sd_upload_rtt_commit_count,
        snapshot.sd_upload_rtt_commit_ms_total,
        snapshot.sd_upload_rtt_commit_ms_max,
        snapshot.sd_upload_rtt_abort_count,
        snapshot.sd_upload_rtt_abort_ms_total,
        snapshot.sd_upload_rtt_abort_ms_max,
        snapshot.sd_upload_rtt_mkdir_count,
        snapshot.sd_upload_rtt_mkdir_ms_total,
        snapshot.sd_upload_rtt_mkdir_ms_max,
        snapshot.sd_upload_rtt_remove_count,
        snapshot.sd_upload_rtt_remove_ms_total,
        snapshot.sd_upload_rtt_remove_ms_max,
    );
    let _ = uart_write_all(uart, upload_rtt_line.as_bytes()).await;

    write_metrics_net_lines(uart).await;
}

pub(super) async fn write_metrics_net_lines(uart: &mut SerialUart) {
    let snapshot = telemetry::snapshot();
    let ip = snapshot.upload_http_ipv4.unwrap_or([0, 0, 0, 0]);

    let mut net_line = heapless::String::<160>::new();
    let _ = write!(
        &mut net_line,
        "METRICS NET wifi_connected={} http_listening={} ip={}.{}.{}.{}\r\n",
        if snapshot.wifi_link_connected { 1 } else { 0 },
        if snapshot.upload_http_listening { 1 } else { 0 },
        ip[0],
        ip[1],
        ip[2],
        ip[3],
    );
    let _ = uart_write_all(uart, net_line.as_bytes()).await;

    let mut liveness_line = heapless::String::<224>::new();
    let _ = write!(
        &mut liveness_line,
        "METRICS LIVENESS accept_link_reset={} health={} wifi_watchdog_disc={}\r\n",
        snapshot.upload_http_accept_link_resets,
        snapshot.upload_http_health_requests,
        snapshot.wifi_connected_watchdog_disconnects,
    );
    let _ = uart_write_all(uart, liveness_line.as_bytes()).await;

    let mut boot_line = heapless::String::<96>::new();
    let _ = write!(
        &mut boot_line,
        "METRICS BOOT reset_code={}\r\n",
        snapshot.boot_reset_reason_code,
    );
    let _ = uart_write_all(uart, boot_line.as_bytes()).await;

    let mut net_pipeline_line = heapless::String::<512>::new();
    let _ = write!(
        &mut net_pipeline_line,
        "METRICS NET_PIPELINE dhcp_wait_n={} dhcp_wait_ms={} dhcp_wait_ms_max={} dhcp_ready={} gate_wifi_down={} gate_link_down={} gate_no_ipv4={} listener_on={} listener_off={} accept_wait_n={} accept_wait_ms={} accept_wait_ms_max={}\r\n",
        snapshot.net_pipeline_dhcp_wait_count,
        snapshot.net_pipeline_dhcp_wait_ms_total,
        snapshot.net_pipeline_dhcp_wait_ms_max,
        snapshot.net_pipeline_dhcp_ready_count,
        snapshot.net_pipeline_gate_wifi_down,
        snapshot.net_pipeline_gate_link_down,
        snapshot.net_pipeline_gate_no_ipv4,
        snapshot.net_pipeline_listener_on,
        snapshot.net_pipeline_listener_off,
        snapshot.net_pipeline_accept_wait_count,
        snapshot.net_pipeline_accept_wait_ms_total,
        snapshot.net_pipeline_accept_wait_ms_max,
    );
    let _ = uart_write_all(uart, net_pipeline_line.as_bytes()).await;

    let mut net_accept_line = heapless::String::<320>::new();
    let _ = write!(
        &mut net_accept_line,
        "METRICS NET_ACCEPT arm_gap_n={} arm_gap_us={} arm_gap_us_max={} arm_gap_after_mkdir_n={} arm_gap_after_mkdir_us={} arm_gap_after_mkdir_us_max={}\r\n",
        snapshot.net_pipeline_accept_arm_gap_count,
        snapshot.net_pipeline_accept_arm_gap_us_total,
        snapshot.net_pipeline_accept_arm_gap_us_max,
        snapshot.net_pipeline_accept_arm_gap_after_mkdir_count,
        snapshot.net_pipeline_accept_arm_gap_after_mkdir_us_total,
        snapshot.net_pipeline_accept_arm_gap_after_mkdir_us_max,
    );
    let _ = uart_write_all(uart, net_accept_line.as_bytes()).await;
}

pub(super) async fn write_telemetry_status_line(uart: &mut SerialUart) {
    let mask = telemetry::diag_mask();
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "TELEM mask=0x{:02x} wifi={} reassoc={} net={} http={} sd={}\r\n",
        mask,
        on_off(mask, telemetry::DIAG_DOMAIN_WIFI),
        on_off(mask, telemetry::DIAG_DOMAIN_REASSOC),
        on_off(mask, telemetry::DIAG_DOMAIN_NET),
        on_off(mask, telemetry::DIAG_DOMAIN_HTTP),
        on_off(mask, telemetry::DIAG_DOMAIN_SD),
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn run_telemetry_set_command(
    uart: &mut SerialUart,
    operation: TelemetrySetOperation,
) {
    let mask = match operation {
        TelemetrySetOperation::Domain { domain, enabled } => {
            telemetry::diag_set_domain(telemetry_domain_mask(domain), enabled)
        }
        TelemetrySetOperation::All { enabled } => {
            telemetry::diag_set_mask(if enabled { telemetry::DIAG_MASK_ALL } else { 0 })
        }
        TelemetrySetOperation::Default => telemetry::diag_set_mask(telemetry::DIAG_MASK_DEFAULT),
    };

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "TELEMSET OK mask=0x{:02x} wifi={} reassoc={} net={} http={} sd={}\r\n",
        mask,
        on_off(mask, telemetry::DIAG_DOMAIN_WIFI),
        on_off(mask, telemetry::DIAG_DOMAIN_REASSOC),
        on_off(mask, telemetry::DIAG_DOMAIN_NET),
        on_off(mask, telemetry::DIAG_DOMAIN_HTTP),
        on_off(mask, telemetry::DIAG_DOMAIN_SD),
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

fn telemetry_domain_mask(domain: TelemetryDomain) -> u32 {
    match domain {
        TelemetryDomain::Wifi => telemetry::DIAG_DOMAIN_WIFI,
        TelemetryDomain::Reassoc => telemetry::DIAG_DOMAIN_REASSOC,
        TelemetryDomain::Net => telemetry::DIAG_DOMAIN_NET,
        TelemetryDomain::Http => telemetry::DIAG_DOMAIN_HTTP,
        TelemetryDomain::Sd => telemetry::DIAG_DOMAIN_SD,
    }
}

fn on_off(mask: u32, domain: u32) -> &'static str {
    if (mask & domain) != 0 {
        "on"
    } else {
        "off"
    }
}
