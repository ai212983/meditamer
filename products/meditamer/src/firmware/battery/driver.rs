//! The BQ27441 acquisition driver (ADR-0018 plan, Phase 5), split out of
//! `battery.rs` (now `battery/mod.rs`) so it can be generic over
//! `embedded_hal_async::i2c::I2c` -- like `inkplate_tempera::fuel_gauge::
//! read_state_of_charge` already is -- and over its own diagnostics sink
//! and wake operation, rather than the concrete `SharedI2cDevice`/
//! `console::println!`/`PcalExpander` production uses.
//!
//! This is what makes "exercise wake failure, read failure, invalid
//! percentage, repeated failure, and recovery through the actual
//! acquisition path" possible on host at all. Two things production
//! depends on are substituted in its focused host tests: UART output and
//! the expander wake operation. [`BatteryDiagnostics`] abstracts the first;
//! [`FuelGaugeWake`] abstracts the second. Production wires the real `SharedI2cDevice` and a
//! `console::println!`-backed `BatteryDiagnostics` impl plus a
//! `PcalExpander`-backed `FuelGaugeWake` impl in `mod.rs`; the host test
//! harness (`tools/touch_replay/tests/battery_driver.rs`) substitutes fakes
//! for both, running this exact `acquire()` body -- same branching, same
//! error construction, same "log once per failed attempt" call site --
//! under test. The shared expander now also has its own host-side owner tests;
//! this driver keeps the narrow wake abstraction to isolate acquisition errors.

use embedded_hal_async::i2c::I2c;

use observation::runtime::AcquisitionDriver;
use observation::time::Duration as ObsDuration;

use super::types::{BatteryFields, BatterySnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryError<E> {
    WakeFailed,
    OutOfRange(u16),
    Read(E),
}

/// Where `BqDriver::acquire()` reports a failed attempt -- "Log concrete
/// acquisition failures once per failed attempt before the runtime
/// discards the driver error," the plan's own subscription-contract
/// bullet. Exactly one call site in `acquire()` below invokes this, and
/// only on the `Err` branch, so "exactly one diagnostic per failed
/// attempt, none while idle or successful" is a property of the control
/// flow itself, not something a caller has to get right separately.
pub trait BatteryDiagnostics<E> {
    fn acquire_failed(&mut self, error: &BatteryError<E>);
}

/// The one operation `BqDriver` needs from the shared PCAL6416A expander
/// lock -- see this module's own doc comment for why it is a trait rather
/// than `inkplate_tempera::expander::PcalExpander` directly. The wake
/// error is never inspected by `acquire()` beyond mapping it to
/// [`BatteryError::WakeFailed`] (matching production's original
/// `.map_err(|_| BatteryError::WakeFailed)`), so it carries no bound of
/// its own.
#[allow(async_fn_in_trait)]
pub trait FuelGaugeWake {
    type Error;

    async fn wake_fuel_gauge(&mut self) -> Result<(), Self::Error>;
}

pub struct BqDriver<I2C, Wake, Diag> {
    i2c: I2C,
    wake: Wake,
    diagnostics: Diag,
}

impl<I2C, Wake, Diag> BqDriver<I2C, Wake, Diag> {
    pub const fn new(i2c: I2C, wake: Wake, diagnostics: Diag) -> Self {
        Self {
            i2c,
            wake,
            diagnostics,
        }
    }
}

impl<I2C, Wake, Diag> AcquisitionDriver<BatteryFields, BatterySnapshot>
    for BqDriver<I2C, Wake, Diag>
where
    I2C: I2c,
    Wake: FuelGaugeWake,
    Diag: BatteryDiagnostics<I2C::Error>,
{
    type Error = BatteryError<I2C::Error>;

    async fn acquire(
        &mut self,
        _requested: BatteryFields,
    ) -> Result<(BatteryFields, BatterySnapshot), Self::Error> {
        let result = async {
            self.wake
                .wake_fuel_gauge()
                .await
                .map_err(|_| BatteryError::WakeFailed)?;
            let soc = inkplate_tempera::fuel_gauge::read_state_of_charge(&mut self.i2c)
                .await
                .map_err(|error| BatteryError::Read(error.0))?;
            // Matches the original `BatteryTick` handler's guard: an
            // out-of-range reading is discarded, not clamped.
            if soc > 100 {
                return Err(BatteryError::OutOfRange(soc));
            }
            Ok((BatteryFields::LEVEL, BatterySnapshot { percent: soc as u8 }))
        }
        .await;
        // `ProviderLoop` consumes the concrete error and only retains the
        // last successful snapshot -- this is the one place that error is
        // observable at all, so it is logged here, once per failed
        // attempt, before it is discarded.
        if let Err(error) = &result {
            self.diagnostics.acquire_failed(error);
        }
        result
    }

    async fn cancel(&mut self) -> Result<(), Self::Error> {
        // Nothing held across an interrupted read: the wake and the
        // fuel-gauge read each complete or fail atomically from this
        // driver's point of view, and the expander lock is never held
        // across a suspension boundary.
        Ok(())
    }

    fn min_acquisition_interval(&self) -> ObsDuration {
        ObsDuration::from_secs(2)
    }
}
