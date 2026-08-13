use core::fmt::Write;

use crate::firmware::{observability, types::SerialUart};

use super::write_line;

pub(super) async fn write_metrics_net_lines(uart: &mut SerialUart) {
    let snapshot = observability::snapshot();
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
    write_line(uart, net_line).await;

    let mut liveness_line = heapless::String::<224>::new();
    let _ = write!(
        &mut liveness_line,
        "METRICS LIVENESS accept_link_reset={} health={} wifi_watchdog_disc={}\r\n",
        snapshot.upload_http_accept_link_resets,
        snapshot.upload_http_health_requests,
        snapshot.wifi_connected_watchdog_disconnects,
    );
    write_line(uart, liveness_line).await;

    let mut boot_line = heapless::String::<96>::new();
    let _ = write!(
        &mut boot_line,
        "METRICS BOOT reset_code={}\r\n",
        snapshot.boot_reset_reason_code,
    );
    write_line(uart, boot_line).await;

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
    write_line(uart, net_pipeline_line).await;

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
    write_line(uart, net_accept_line).await;
}
