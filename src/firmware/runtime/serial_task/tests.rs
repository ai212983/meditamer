use super::{commands::SdWaitTarget, parser::SDWAIT_DEFAULT_TIMEOUT_MS, *};

use super::super::super::app_state::{
    AppStateCommand, BaseMode, DayBackground, DiagKind, OverlayMode,
};
use super::super::super::types::{AppEvent, SD_PATH_MAX, SD_WRITE_MAX};
use super::commands::{
    app_state_command_for_serial, StateSetOperation, TelemetryDomain, TelemetrySetOperation,
};

fn path_from(buf: &[u8; SD_PATH_MAX], len: u8) -> &str {
    core::str::from_utf8(&buf[..len as usize]).unwrap()
}

include!("tests/parse_core.rs");
include!("tests/netcfg.rs");
include!("tests/sdwait_storage.rs");
include!("tests/mappings.rs");
