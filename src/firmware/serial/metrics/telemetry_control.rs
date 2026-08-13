use core::fmt::Write;

use crate::firmware::{observability, types::SerialUart};

use super::{on_off, telemetry_domain_mask, write_line, TelemetrySetOperation};

pub(super) async fn write_telemetry_status_line(uart: &mut SerialUart) {
    let mask = observability::log_filter_mask();
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "TELEM mask=0x{:02x} wifi={} reassoc={} net={} http={} sd={}\r\n",
        mask,
        on_off(mask, observability::LOG_DOMAIN_WIFI),
        on_off(mask, observability::LOG_DOMAIN_REASSOC),
        on_off(mask, observability::LOG_DOMAIN_NET),
        on_off(mask, observability::LOG_DOMAIN_HTTP),
        on_off(mask, observability::LOG_DOMAIN_SD),
    );
    write_line(uart, line).await;
}

pub(super) async fn run_telemetry_set_command(
    uart: &mut SerialUart,
    operation: TelemetrySetOperation,
) {
    let mask = match operation {
        TelemetrySetOperation::Domain { domain, enabled } => {
            observability::set_log_filter_domain(telemetry_domain_mask(domain), enabled)
        }
        TelemetrySetOperation::All { enabled } => observability::set_log_filter_mask(if enabled {
            observability::LOG_FILTER_MASK_ALL
        } else {
            0
        }),
        TelemetrySetOperation::Default => {
            observability::set_log_filter_mask(observability::LOG_FILTER_MASK_DEFAULT)
        }
    };

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        &mut line,
        "TELEMSET OK mask=0x{:02x} wifi={} reassoc={} net={} http={} sd={}\r\n",
        mask,
        on_off(mask, observability::LOG_DOMAIN_WIFI),
        on_off(mask, observability::LOG_DOMAIN_REASSOC),
        on_off(mask, observability::LOG_DOMAIN_NET),
        on_off(mask, observability::LOG_DOMAIN_HTTP),
        on_off(mask, observability::LOG_DOMAIN_SD),
    );
    write_line(uart, line).await;
}
