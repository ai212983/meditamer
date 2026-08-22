//! Exclusive-radio arbitration between the Wi-Fi supervisor and the BLE stack.
//!
//! See the crate manifest for why this exists as its own crate.

#![no_std]

pub mod claim;
pub mod handoff;
