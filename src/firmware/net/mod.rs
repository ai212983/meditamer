//! Networking.
//!
//! [`wifi`] owns the radio: the driver, scanning, the connect state machine, and
//! its retry policy. [`runtime`] brings up the Embassy network stack over it.
//! Consumers -- today only the asset-upload HTTP server -- live elsewhere.

pub(crate) mod handoff;
mod runtime;
pub(crate) mod wifi;

#[cfg(feature = "ble-foundation")]
pub(crate) use handoff::phase1s_exclusive_ownership_confirmed;
#[cfg(feature = "ble-foundation")]
pub(crate) use runtime::{
    cancel_handoff_request, exclusive_lease_matches, receive_handoff_ack, request_handoff,
    residency_snapshot,
};
pub(crate) use runtime::{network_owner_task, stack_resources};
pub(crate) use wifi::boot_scan_only_diag_enabled;
