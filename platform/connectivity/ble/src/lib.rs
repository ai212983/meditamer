//! Shared BLE runtime (shared BLE runtime and roles plan,
//! `docs/plans/ble/README.md`).
//!
//! [`probe`] is Phase 1's thin controller start/stop runner: bring the
//! reviewed vendor BLE controller source up through `esp-radio` and
//! immediately tear it back down, proving one controller instance survives
//! start then stop on both `targets/meditamer-inkplate` (ESP32) and
//! `targets/medinote-waveshare` (ESP32-S3) before scanning, GATT, or HID
//! land. It needs the Xtensa toolchain and the reviewed radio source, so it
//! only builds `#[cfg(target_os = "none")]`.
//!
//! Phase 2 adds the shared boundary: [`capacity`]'s explicit bounds,
//! [`scan`]'s bounded, restartable result set, [`gatt`]'s bounded connection
//! lifecycle with deadlines, [`hid`]'s bounded descriptor discovery and
//! model-neutral report decoding, [`telemetry`]'s counters, and [`input`]'s
//! latest-`GamepadSample`-plus-bounded-edge-queue publication with
//! clear-and-reconcile recovery (decision D-004). Every one of those is
//! host-testable: `cfg(any(target_os = "none", feature = "host-tests"))`,
//! and none of them read a real clock -- callers supply the current tick,
//! so restart, deadline, full-capacity, stale-event, and
//! neutral-input-after-failure behavior is deterministic under `cargo test`.
//!
//! Chip setup, the BLE peripheral singleton, and any allocator or executor
//! the radio needs remain each `targets/*` crate's responsibility (see the
//! plan's ownership table); this crate owns the radio session, roles,
//! GATT/HID handling, and telemetry above that.
//!
//! Phase 4 adds the peripheral role's first shared primitives, extracted
//! from Meditamer's existing `ble-foundation` diagnostic probe rather than
//! designed fresh: [`reusable_slot`]'s single-owner static storage (needed
//! because a peripheral cycle rebuilds its `Stack` every window, and
//! `static`s cannot be reinitialized), [`teardown`]'s pure post-window
//! queue/transport invariant checks (host-testable), and
//! [`packet_pool`]'s bounded `PacketPool` implementation (chip-only, see
//! its own doc comment for why). [`diagnostic`] adds ADR-0011's wire schema
//! (`BuildInfo`/`LifecycleStatus` encode/decode, `EchoRateLimiter`) and
//! [`address`] its random-static address rotation; both are now wired into
//! the product's diagnostic peripheral, along with [`deadline`]'s
//! `VisibilityWindow` (ADR-0011's advertising/connection/idle/window
//! deadlines -- the teardown-acknowledgement deadline is not this module's
//! concern, see its own doc comment for why). See the plan and its ledger
//! for status.
//!
//! [ADR-0016](../../../../docs/architecture/0016-ble-central-pairing-and-bonding.md)
//! adds pairing and bonding to the central role: [`bond`]'s host-testable,
//! bounded [`bond::BondTable`] and its fixed record layout, chip-free and
//! `trouble-host`-free like every other module here.
//!
//! Targets enable `central` for the live scan, connection, diagnostic-client,
//! and pairing adapters, or `peripheral` for a peripheral host. Either host
//! role exposes the bounded packet pool. Chip selection alone keeps the
//! host-independent primitives and controller start/stop probe available.

#![no_std]

#[cfg(all(target_os = "none", feature = "central"))]
mod diagnostic_client;
#[cfg(all(target_os = "none", feature = "central"))]
pub use diagnostic_client::{
    run as run_diagnostic_client, DiagnosticClientOutcome, DiagnosticClientReport,
};
#[cfg(all(target_os = "none", feature = "central"))]
mod live_connect;
#[cfg(all(target_os = "none", feature = "central"))]
pub use live_connect::{
    run as run_live_connect, run_with_resources as run_live_connect_with_resources,
    DiscoveredCharacteristic, DiscoveredService, DiscoveredUuid, HidInputSink, LiveConnectOptions,
    LiveConnectOutcome, LiveConnectReport, Resources as LiveConnectResources,
    ServiceDiscoveryDiagnostic, ServiceDiscoveryFailure,
};
#[cfg(all(target_os = "none", feature = "central"))]
mod live_scan;
#[cfg(all(target_os = "none", feature = "central"))]
pub use live_scan::{run as run_live_scan, LiveScanOutcome, LiveScanReport, LiveScanWindowResult};
#[cfg(all(target_os = "none", feature = "central"))]
pub mod pairing;
#[cfg(target_os = "none")]
mod probe;
#[cfg(target_os = "none")]
pub use probe::{start_stop_probe, StartStopOutcome, StartStopResult};

#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod address;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod bond;
pub mod capacity;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod deadline;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod diagnostic;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod gatt;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod hid;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod input;
#[cfg(all(target_os = "none", any(feature = "central", feature = "peripheral")))]
pub mod packet_pool;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod reusable_slot;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod scan;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod teardown;
#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod telemetry;
