//! Network-owned telemetry: link, IPv4, listener, admission, and
//! network-pipeline counters, plus their snapshot.
//!
//! [`counters`] holds the raw atomics, [`wifi`]/[`listener`] the write side,
//! [`snapshot`] the read side a product's diagnostics compose with its own.
//! Split out of `products/meditamer/src/firmware/observability` (product/
//! target axis completion plan, Phase 4): HTTP request/body/SD-roundtrip
//! timing stayed product-side, since the product's upload service owns that
//! work, not the network runtime.

mod counters;
mod helpers;
mod listener;
mod snapshot;
mod types;
mod wifi;

pub use listener::{
    record_net_pipeline_accept_arm_gap, record_net_pipeline_accept_wait,
    record_net_pipeline_dhcp_ready, record_net_pipeline_dhcp_wait, record_net_pipeline_gate,
    set_upload_http_listener,
};
pub use snapshot::{snapshot, NetSnapshot};
pub use types::{NetPipelineGate, WifiScanPhase};
pub use wifi::{
    record_wifi_connect_attempt, record_wifi_connect_failure, record_wifi_connect_success,
    record_wifi_link_rssi, record_wifi_reassoc_auth_rotation, record_wifi_reassoc_channel_probe,
    record_wifi_reassoc_config_applied, record_wifi_reassoc_connect_begin,
    record_wifi_reassoc_connect_failure_detail, record_wifi_reassoc_connect_success,
    record_wifi_reassoc_credentials_changed, record_wifi_reassoc_credentials_received,
    record_wifi_reassoc_disconnect_event, record_wifi_reassoc_hint_retry,
    record_wifi_reassoc_mode_pause, record_wifi_reassoc_mode_resume, record_wifi_reassoc_scan,
    record_wifi_reassoc_stage, record_wifi_reassoc_start_err, record_wifi_reassoc_start_ok,
    record_wifi_scan, record_wifi_watchdog_disconnect, set_wifi_ipv4, set_wifi_link_connected,
    wifi_link_connected,
};
