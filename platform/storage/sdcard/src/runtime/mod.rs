//! SD runtime helpers driven by the firmware tasks.
//!
//! [`probe_rw`] runs the probe and read/write verification sequences;
//! [`common`] holds the result and power-mode vocabulary they report in.

mod common;
mod probe_rw;

pub use common::{SdPowerMode, SdRuntimeResultCode};
pub use probe_rw::{run_sd_probe, run_sd_rw_verify, SdPowerAction};
