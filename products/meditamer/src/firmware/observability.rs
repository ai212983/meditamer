//! Firmware observability: passive counters, snapshots, and log filtering.
//!
//! [`counters`] holds the raw atomics, [`recorders`] the write side each
//! subsystem calls, [`snapshot`] the read side the serial console reports, and
//! [`types`] the vocabulary they share.
//!
//! Network-owned counters (Wi-Fi connect/reassociation, link, IPv4, listener,
//! network-pipeline) and the shared log-filter registry moved to
//! `platform/connectivity/netstack`/`platform/diagnostics/runtime` in the product/target axis
//! completion plan's Phase 4. [`snapshot`] composes the moved-out network
//! snapshot back into this module's [`Snapshot`], so the serial metric names
//! and values this module has always reported stay unchanged; the log-domain
//! constants and mask operations are re-exported from `runtime` directly,
//! since both this module's own SD/HTTP diagnostics and netstack's Wi-Fi/net
//! diagnostics filter through that one shared registry.

mod counters;
mod recorders;
mod snapshot;
mod types;

pub use recorders::*;
pub(crate) use runtime::{
    log_filter_enabled, log_filter_mask, set_log_filter_domain, set_log_filter_mask,
    LOG_DOMAIN_HTTP, LOG_DOMAIN_NET, LOG_DOMAIN_REASSOC, LOG_DOMAIN_SD, LOG_DOMAIN_WIFI,
    LOG_FILTER_MASK_ALL, LOG_FILTER_MASK_DEFAULT,
};
pub(crate) use snapshot::snapshot;
pub(crate) use types::Snapshot;
#[cfg(feature = "asset-upload-http")]
pub(crate) use types::{SdUploadRoundtripPhase, UploadHttpPhaseMetrics};
