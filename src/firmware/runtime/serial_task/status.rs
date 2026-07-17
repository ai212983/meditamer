use core::fmt::Write;

use crate::firmware::types::SerialStatusEvent;

pub(super) fn format_status(event: SerialStatusEvent) -> heapless::String<128> {
    let mut line = heapless::String::new();
    match event {
        SerialStatusEvent::ImuScheduler {
            sensor_odr_hz,
            idle_hz,
            active_hz,
            active_hold_ms,
        } => {
            let _ = writeln!(
                line,
                "imu: scheduler odr_hz={sensor_odr_hz} idle_hz={idle_hz} active_hz={active_hz} hold_ms={active_hold_ms}\r"
            );
        }
        SerialStatusEvent::ImuReady => {
            let _ = line.push_str("imu: ready\r\n");
        }
        SerialStatusEvent::ImuInitFailed => {
            let _ = line.push_str("imu: init_failed; retrying\r\n");
        }
        SerialStatusEvent::ImuReadError => {
            let _ = line.push_str("imu: read_error; retrying\r\n");
        }
    }
    line
}
