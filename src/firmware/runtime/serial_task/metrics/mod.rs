use crate::firmware::{telemetry, touch::debug_log::uart_write_all, types::SerialUart};

use super::commands::{TelemetryDomain, TelemetrySetOperation};

mod metrics_dump;
mod metrics_imu;
mod metrics_net;
mod telemetry_control;

async fn write_line<const N: usize>(uart: &mut SerialUart, line: heapless::String<N>) {
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

pub(super) async fn write_metrics_lines(uart: &mut SerialUart) {
    metrics_dump::write_metrics_lines(uart).await;
}

pub(super) async fn write_metrics_net_lines(uart: &mut SerialUart) {
    metrics_net::write_metrics_net_lines(uart).await;
}

pub(super) async fn write_telemetry_status_line(uart: &mut SerialUart) {
    telemetry_control::write_telemetry_status_line(uart).await;
}

pub(super) async fn run_telemetry_set_command(
    uart: &mut SerialUart,
    operation: TelemetrySetOperation,
) {
    telemetry_control::run_telemetry_set_command(uart, operation).await;
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

use metrics_imu::write_metrics_imu_line;
