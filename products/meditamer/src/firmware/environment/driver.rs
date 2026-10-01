//! Wraps `InkplateEnvironmentDriver` with the recoverable-after-failure
//! state the plan's Phase 4 asks for: an initialization failure (or a later
//! sample failure) drops the driver rather than wedging it, and the next
//! `acquire` call re-initializes from a freshly constructed device handle on
//! the shared bus -- `observation::runtime`'s own retry backoff (doubling
//! from [`super::config::ENVIRONMENT_RETRY_FLOOR_MS`]) governs how soon that
//! retry happens, "under the established bus arbitration policy" the plan
//! asks for rather than a bespoke one of this driver's own.

use embedded_hal_async::i2c::ErrorType;
use inkplate_tempera::environment::{EnvironmentError, InkplateEnvironment};
use inkplate_tempera::sht45::Sht45;
use observation::field::FieldMask;
use observation::runtime::AcquisitionDriver;
use observation::time::Duration as ObsDuration;

use crate::firmware::types::{
    shared_i2c_device, InkplateEnvironmentDriver, SharedI2cBus, SharedI2cDevice,
};

use super::config::ENVIRONMENT_RETRY_FLOOR_MS;
use super::types::{EnvironmentFields, EnvironmentSnapshot};

pub(crate) struct BmeDriver {
    bus: &'static SharedI2cBus,
    sensor: Option<InkplateEnvironmentDriver>,
    external_present: Option<bool>,
}

impl BmeDriver {
    /// `initial` is the result of the one-shot pre-touch-startup attempt
    /// `run_board_runtime` still makes at the same point it always did (see
    /// that function's own comment on why BME688 goes first) -- `Some` if it
    /// already succeeded, `None` if it is still to be attempted (or
    /// retried) from here, using `bus` to construct a fresh device handle
    /// each time since a failed `InkplateEnvironment::initialize` consumes
    /// the one it was given.
    pub(crate) const fn new(
        initial: Option<InkplateEnvironmentDriver>,
        bus: &'static SharedI2cBus,
    ) -> Self {
        Self {
            bus,
            sensor: initial,
            external_present: None,
        }
    }
}

impl AcquisitionDriver<EnvironmentFields, EnvironmentSnapshot> for BmeDriver {
    type Error = EnvironmentError<<SharedI2cDevice as ErrorType>::Error>;

    async fn acquire(
        &mut self,
        _requested: EnvironmentFields,
    ) -> Result<(EnvironmentFields, EnvironmentSnapshot), Self::Error> {
        if self.sensor.is_none() {
            self.sensor = Some(
                InkplateEnvironment::initialize(shared_i2c_device(self.bus))
                    .await
                    .inspect_err(|error| {
                        console::println!("BME688_ACQUIRE failed=initialize error={:?}", error);
                    })?,
            );
        }
        // Present after the block above, whichever branch ran.
        let sensor = self.sensor.as_mut().expect("initialized just above");
        match sensor.sample().await {
            Ok(onboard) => {
                let external = self.sample_external().await;
                Ok((
                    EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
                    EnvironmentSnapshot { onboard, external },
                ))
            }
            Err(error) => {
                // A sampling failure might mean the part needs a fresh
                // reset/configure sequence, not just a retried read -- drop
                // it so the next `acquire` re-initializes, the same
                // recovery `imu_acquisition_task` uses on a read failure.
                self.sensor = None;
                console::println!("BME688_ACQUIRE failed=sample error={:?}", error);
                Err(error)
            }
        }
    }

    async fn cancel(&mut self) -> Result<(), Self::Error> {
        // BME68x returns to sleep on its own after a forced-mode conversion
        // completes; there is no separate low-power command to issue and no
        // bus state a cancellation needs to release.
        Ok(())
    }

    fn min_acquisition_interval(&self) -> ObsDuration {
        ObsDuration::from_millis(ENVIRONMENT_RETRY_FLOOR_MS)
    }
}

impl BmeDriver {
    async fn sample_external(
        &mut self,
    ) -> Option<inkplate_tempera::environment::EnvironmentReading> {
        let mut sensor = Sht45::new(shared_i2c_device(self.bus));
        match sensor.sample().await {
            Ok(reading) => {
                if self.external_present != Some(true) {
                    console::println!("SHT45 status=ready address=0x44");
                }
                self.external_present = Some(true);
                Some(reading)
            }
            Err(error) => {
                if self.external_present != Some(false) {
                    console::println!("SHT45 status=unavailable error={:?}", error);
                }
                self.external_present = Some(false);
                None
            }
        }
    }
}
