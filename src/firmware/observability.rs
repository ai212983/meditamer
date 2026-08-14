//! Firmware observability: passive counters, snapshots, and log filtering.
//!
//! [`counters`] holds the raw atomics, [`recorders`] the write side each
//! subsystem calls, [`snapshot`] the read side the serial console reports, and
//! [`types`] the vocabulary they share.

mod counters;
mod recorders;
mod snapshot;
mod types;

pub(crate) use counters::{
    LOG_DOMAIN_HTTP, LOG_DOMAIN_NET, LOG_DOMAIN_REASSOC, LOG_DOMAIN_SD, LOG_DOMAIN_WIFI,
    LOG_FILTER_MASK_ALL, LOG_FILTER_MASK_DEFAULT,
};
pub(crate) use recorders::*;
pub(crate) use snapshot::snapshot;
pub(crate) use types::Snapshot;
#[cfg(feature = "asset-upload-http")]
pub(crate) use types::{
    NetPipelineGate, SdUploadRoundtripPhase, UploadHttpPhaseMetrics, WifiScanPhase,
};
