//! Networking.
//!
//! [`wifi`] owns the radio: the driver, scanning, the connect state machine, and
//! its retry policy. [`runtime`] brings up the Embassy network stack over it.
//! Consumers -- today only the asset-upload HTTP server -- live elsewhere.

pub(crate) mod host;
mod runtime;
pub(crate) mod wifi;

#[cfg(feature = "ble-foundation")]
pub(crate) use runtime::{cancel_handoff_request, receive_handoff_ack, request_handoff};
pub(crate) use runtime::{run_network_owner, stack_resources, NET_STACK_SOCKETS};
pub(crate) use wifi::boot_scan_only_diag_enabled;
