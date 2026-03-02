use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{with_timeout, Duration, Timer};

use crate::firmware::{
    app_state::{AppStateDiagControl, DiagKind},
    config::{DIAG_CONTROL_EVENTS, SD_DIAG_RESULTS, SD_REQUESTS},
    telemetry,
    types::{SdCommand, SdRequest, SdResult},
};

include!("diagnostics/model.rs");
include!("diagnostics/control.rs");
include!("diagnostics/sd_checks.rs");
include!("diagnostics/wifi.rs");
