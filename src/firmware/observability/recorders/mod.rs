//! Observability recording entry points.
//!
//! One module per subsystem -- Wi-Fi, upload/network, SD upload, stack
//! headroom -- plus the log-filter mask that gates their logging.

#[cfg(feature = "asset-upload-http")]
mod helpers;
mod log_filter;
mod sd_upload;
mod stack;
#[cfg(feature = "asset-upload-http")]
mod upload_net;
#[cfg(feature = "asset-upload-http")]
mod wifi;

pub(crate) use log_filter::{
    log_filter_enabled, log_filter_mask, set_log_filter_domain, set_log_filter_mask,
};
pub(crate) use sd_upload::set_boot_reset_reason_code;
#[cfg(feature = "asset-upload-http")]
pub(crate) use sd_upload::{
    record_sd_upload_roundtrip_code, record_sd_upload_roundtrip_timeout,
    record_sd_upload_roundtrip_timing, record_sd_upload_session_mode_off_abort,
    record_sd_upload_session_timeout_abort,
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
