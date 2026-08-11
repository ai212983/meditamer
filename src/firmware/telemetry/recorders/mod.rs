//! Telemetry recording entry points.
//!
//! One module per subsystem -- Wi-Fi, upload/network, SD upload, stack
//! headroom -- plus the diagnostic-domain mask that gates their logging.

mod diag_mask;
#[cfg(feature = "asset-upload-http")]
mod helpers;
mod sd_upload;
mod stack;
#[cfg(feature = "asset-upload-http")]
mod upload_net;
#[cfg(feature = "asset-upload-http")]
mod wifi;

pub(crate) use diag_mask::{diag_enabled, diag_mask, diag_set_domain, diag_set_mask};
pub(crate) use sd_upload::set_boot_reset_reason_code;
#[cfg(feature = "asset-upload-http")]
pub(crate) use sd_upload::{
    record_sd_upload_roundtrip_code, record_sd_upload_roundtrip_timeout,
    record_sd_upload_roundtrip_timing, record_sd_upload_session_mode_off_abort,
    record_sd_upload_session_timeout_abort, record_wifi_watchdog_disconnect,
    set_upload_http_listener, set_wifi_link_connected, wifi_link_connected,
};
pub(crate) use stack::{
    configure_touch_core_stack, log_stack_headroom, minimum_stack_headroom_bytes,
    minimum_touch_core_stack_headroom_bytes, record_stack_headroom,
    record_touch_core_stack_headroom,
};
#[cfg(feature = "asset-upload-http")]
pub(crate) use upload_net::*;
#[cfg(feature = "asset-upload-http")]
pub(crate) use wifi::*;
