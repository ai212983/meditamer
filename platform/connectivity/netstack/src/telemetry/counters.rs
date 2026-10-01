//! Network telemetry counters.
//!
//! Plain atomics: [`super::wifi`]/[`super::listener`] write them,
//! [`super::snapshot`] reads them. Nothing here interprets a value. Split out
//! of `products/meditamer/src/firmware/observability/counters.rs`
//! (product/target axis completion plan, Phase 4) on the real ownership
//! boundary: link, IPv4, listener, admission, and network-pipeline counters
//! are network-owned; HTTP request/body/SD-roundtrip timing stayed with the
//! product's upload service.

use core::sync::atomic::{AtomicBool, AtomicU32};

pub(super) static WIFI_CONNECT_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_CONNECT_SUCCESSES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_CONNECT_FAILURES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASON_NO_AP_FOUND: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_SCAN_RUNS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_SCAN_EMPTY: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_SCAN_TARGET_HITS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_CONNECTED_WATCHDOG_DISCONNECTS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_MODE_PAUSES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_MODE_RESUMES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CREDENTIALS_RECEIVED: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CREDENTIALS_CHANGED: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONFIG_APPLIED: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_START_OK: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_START_ERR: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONNECT_BEGIN: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONNECT_SUCCESS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONNECT_FAILURE: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_DISCONNECT_EVENTS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CHANNEL_PROBES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_AUTH_ROTATIONS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_HINT_RETRIES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONNECT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_CONNECT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_ACTIVE_RUNS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_ACTIVE_EMPTY: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_ACTIVE_HITS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_ACTIVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_ACTIVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_PASSIVE_RUNS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_PASSIVE_EMPTY: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_PASSIVE_HITS: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_PASSIVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_SCAN_PASSIVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_2: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_201: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_202: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_203: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_204: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_205: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_210: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_211: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_212: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_REASON_OTHER: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_REASON: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_AUTH_IDX: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_CHANNEL_HINT: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_PROBE_IDX: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_SCAN_CHANNEL: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_REASSOC_LAST_STAGE: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_DHCP_WAIT_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_DHCP_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_DHCP_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_DHCP_READY_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_GATE_WIFI_DOWN: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_GATE_LINK_DOWN: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_GATE_NO_IPV4: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_LISTENER_ON: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_LISTENER_OFF: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_WAIT_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_US_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_US_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static NET_PIPELINE_ACCEPT_ARM_GAP_AFTER_MKDIR_US_MAX: AtomicU32 = AtomicU32::new(0);
// Encoded as dBm+128 (0..255); u32::MAX means "unknown/no samples".
pub(super) static WIFI_LINK_RSSI_LAST_DBM: AtomicU32 = AtomicU32::new(u32::MAX);
pub(super) static WIFI_LINK_RSSI_MIN_DBM: AtomicU32 = AtomicU32::new(u32::MAX);
pub(super) static WIFI_LINK_RSSI_MAX_DBM: AtomicU32 = AtomicU32::new(u32::MAX);
pub(super) static WIFI_LINK_RSSI_SAMPLES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_LINK_RSSI_LOW_SAMPLES: AtomicU32 = AtomicU32::new(0);
pub(super) static WIFI_LINK_CONNECTED: AtomicBool = AtomicBool::new(false);
// DHCP lease owned by the Wi-Fi task. HTTP listener lifecycle must not clear it.
pub(super) static WIFI_IPV4: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_LISTENING: AtomicBool = AtomicBool::new(false);
pub(super) static UPLOAD_HTTP_IPV4: AtomicU32 = AtomicU32::new(0);
