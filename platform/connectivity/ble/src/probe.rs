//! Phase 1's thin controller start/stop runner.
//!
//! Deliberately does not build a `trouble-host` GATT stack or an
//! `ExternalController` the way `products/meditamer`'s existing
//! `ble-foundation` probe does (see its module doc): Phase 1 proves only that
//! the reviewed vendor BLE controller source builds and survives one
//! start/stop cycle on each chip. Host/GATT composition is shared-runtime
//! scope for a later phase, once both targets pass this.

use esp_hal::peripherals::BT;
use esp_hal::time::Instant;
use esp_radio::ble::controller::BleConnector;

/// Why [`start_stop_probe`] did or did not complete both start and stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StartStopOutcome {
    /// The controller started and stopped cleanly.
    Completed,
    /// `BleConnector::new` returned an initialization error.
    ControllerInit,
}

impl StartStopOutcome {
    /// A short, stable label for serial/telemetry output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::ControllerInit => "controller_init_failed",
        }
    }
}

/// Timing and outcome of one [`start_stop_probe`] run.
#[derive(Clone, Copy, Debug)]
pub struct StartStopResult {
    /// What happened.
    pub outcome: StartStopOutcome,
    /// Wall-clock microseconds spent bringing the controller up.
    pub start_micros: u64,
    /// Wall-clock microseconds spent tearing the controller back down.
    /// Zero when [`StartStopOutcome::ControllerInit`] never started it.
    pub stop_micros: u64,
}

/// Bring the BLE controller up and immediately tear it back down.
///
/// Takes the chip's `BT` peripheral singleton by value, same as the existing
/// diagnostic probe: the caller decides how it obtains one (fresh from
/// `esp_hal::init`, or `unsafe { BT::steal() }` after a prior owner's clean
/// teardown), Phase 1 does not.
pub fn start_stop_probe(device: BT<'static>) -> StartStopResult {
    let start_at = Instant::now();
    let connector = match BleConnector::new(device, Default::default()) {
        Ok(connector) => connector,
        Err(_) => {
            return StartStopResult {
                outcome: StartStopOutcome::ControllerInit,
                start_micros: start_at.elapsed().as_micros(),
                stop_micros: 0,
            };
        }
    };
    let start_micros = start_at.elapsed().as_micros();

    let stop_at = Instant::now();
    drop(connector);
    let stop_micros = stop_at.elapsed().as_micros();

    StartStopResult {
        outcome: StartStopOutcome::Completed,
        start_micros,
        stop_micros,
    }
}
