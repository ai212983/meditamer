//! Firmware telemetry.
//!
//! [`counters`] holds the raw atomics, [`recorders`] the write side each
//! subsystem calls, [`snapshot`] the read side the serial console reports, and
//! [`types`] the vocabulary they share.

mod counters;
mod recorders;
mod snapshot;
mod types;

pub(crate) use counters::{
    DIAG_DOMAIN_HTTP, DIAG_DOMAIN_NET, DIAG_DOMAIN_REASSOC, DIAG_DOMAIN_SD, DIAG_DOMAIN_WIFI,
    DIAG_MASK_ALL, DIAG_MASK_DEFAULT,
};
pub(crate) use recorders::*;
pub(crate) use snapshot::snapshot;
#[cfg(feature = "asset-upload-http")]
pub(crate) use types::{
    NetPipelineGate, SdUploadRoundtripPhase, UploadHttpPhaseMetrics, WifiScanPhase,
};
