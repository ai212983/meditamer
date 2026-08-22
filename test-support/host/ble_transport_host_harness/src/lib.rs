#![no_std]
#![allow(dead_code)]

#[path = "../../../../vendor/esp-radio-1.0.0-beta.0-bounded/src/ble/tx_cancellation.rs"]
mod tx_cancellation;

#[path = "../../../../vendor/esp-radio-1.0.0-beta.0-bounded/src/compat/queue_lifecycle.rs"]
mod queue_lifecycle;

// ADR-0015: the handoff model is the `arbitration` crate now, so this harness
// depends on it instead of `#[path]`-including firmware source.
use arbitration::handoff as radio_handoff;

#[path = "../../../../src/firmware/serial/io/netcfg_persistence.rs"]
mod netcfg_persistence;
