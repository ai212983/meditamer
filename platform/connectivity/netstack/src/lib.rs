//! Shared network runtime: Wi-Fi, Embassy networking, configuration, and
//! telemetry (product/target axis completion plan, Phase 4).
//!
//! [`wifi`] owns the radio: the driver, scanning, the connect state machine,
//! and its retry policy. [`runtime`] brings up the Embassy network stack over
//! it and supervises radio-handoff epochs. [`config`] is the complete Wi-Fi
//! configuration domain -- types, validation, channels, and the shared
//! internal-flash credential format/port. [`telemetry`] is the network-owned
//! counters and snapshot. [`host`] is the product/target composition seam:
//! the async service-work bound, the synchronous product-state port, and
//! netstack-owned listener/admission state.
//!
//! No product or board dependency anywhere in this crate (ADR-0015): a
//! product supplies its service work and policy through [`host`]; a target
//! selects the chip and installs the concrete ports before the network task
//! starts.

#![no_std]

pub mod config;
pub mod host;
mod owner;
pub mod telemetry;
pub mod wifi;

#[cfg(feature = "ble-foundation")]
pub use owner::{cancel_handoff_request, receive_handoff_ack, request_handoff};
pub use owner::{
    run_network_owner, run_radio_supervisor, run_radio_supervisor_initially_off, stack_resources,
    RadioOffService, NET_STACK_SOCKETS,
};
pub use wifi::boot_scan_only_diag_enabled;
