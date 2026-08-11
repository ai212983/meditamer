//! Networking.
//!
//! [`wifi`] owns the radio: the driver, scanning, the connect state machine, and
//! its retry policy. [`runtime`] brings up the Embassy network stack over it.
//! Consumers -- today only the asset-upload HTTP server -- live elsewhere.

mod runtime;
pub(crate) mod wifi;

pub(crate) use runtime::{net_task, setup, wifi_connection_task};
pub(crate) use wifi::boot_scan_only_diag_enabled;
