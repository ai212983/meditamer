mod mappings;
mod netcfg;
mod parse_core;
mod sdwait_storage;

use super::{commands::SdWaitTarget, parser::SDWAIT_DEFAULT_TIMEOUT_MS, *};

use super::super::app_state::{AppStateCommand, DiagKind};
use super::super::types::{AppEvent, SerialStatusEvent, SD_PATH_MAX, SD_WRITE_MAX};
use super::commands::{
    app_state_command_for_serial, StateSetOperation, TelemetryDomain, TelemetrySetOperation,
};

fn path_from(buf: &[u8; SD_PATH_MAX], len: u8) -> &str {
    core::str::from_utf8(&buf[..len as usize]).unwrap()
}

#[test]
fn serial_status_lines_preserve_existing_wire_format() {
    assert_eq!(
        status::format_status(SerialStatusEvent::Scheduler {
            sensor_odr_hz: 416,
            idle_hz: 20,
            active_hz: 104,
            active_hold_ms: 750,
        })
        .as_str(),
        "imu: scheduler odr_hz=416 idle_hz=20 active_hz=104 hold_ms=750\r\n"
    );
    assert_eq!(
        status::format_status(SerialStatusEvent::Ready).as_str(),
        "imu: ready\r\n"
    );
    assert_eq!(
        status::format_status(SerialStatusEvent::InitFailed).as_str(),
        "imu: init_failed; retrying\r\n"
    );
    assert_eq!(
        status::format_status(SerialStatusEvent::ReadError).as_str(),
        "imu: read_error; retrying\r\n"
    );
}
