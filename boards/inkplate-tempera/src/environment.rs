//! Board-owned BME688 temperature and humidity acquisition.
//!
//! The driver stays generic over `embedded-hal-async` I2C so the target can
//! give it one device handle on the board's shared bus. Product policy (the
//! sample cadence and where readings appear) remains outside this module.

use bme68x::asynch::{Bme68x, I2cAddress, I2cInterface};
use bme68x::{Configuration, Error as BmeError, Filter, OperationMode, Oversampling, StandbyTime};
use embassy_time::{Delay, Timer};
use embedded_hal_async::i2c::I2c;

const CONFIGURATION: Configuration = Configuration {
    humidity_oversampling: Oversampling::X2,
    temperature_oversampling: Oversampling::X2,
    pressure_oversampling: Oversampling::None,
    filter: Filter::Off,
    standby_time: StandbyTime::None,
};

/// One compensated environmental sample in the BME68x fixed-point units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvironmentReading {
    /// Hundredths of a degree Celsius.
    pub temperature_centidegrees: i16,
    /// Thousandths of a percent relative humidity.
    pub humidity_millipercent: u32,
}

/// Failures that can occur while initializing or sampling the onboard BME688.
#[derive(Debug)]
pub enum EnvironmentError<E> {
    Driver(BmeError<E>),
    NoData,
}

impl<E> From<BmeError<E>> for EnvironmentError<E> {
    fn from(error: BmeError<E>) -> Self {
        Self::Driver(error)
    }
}

/// The Inkplate 4 TEMPERA's onboard BME688.
pub struct InkplateEnvironment<I2C> {
    sensor: Bme68x<I2cInterface<I2C>, Delay>,
}

impl<I2C> InkplateEnvironment<I2C>
where
    I2C: I2c,
{
    /// Reset, identify, and configure the onboard sensor at its board-wired
    /// low I2C address (`0x76`).
    pub async fn initialize(i2c: I2C) -> Result<Self, EnvironmentError<I2C::Error>> {
        let interface = I2cInterface::new(i2c, I2cAddress::Low);
        let mut sensor = Bme68x::new(interface, Delay).await?;
        sensor.set_configuration(&CONFIGURATION).await?;
        sensor.set_operation_mode(OperationMode::Sleep).await?;
        Ok(Self { sensor })
    }

    /// Perform one forced-mode temperature/humidity conversion. The BME688
    /// returns to sleep automatically after the conversion.
    pub async fn sample(&mut self) -> Result<EnvironmentReading, EnvironmentError<I2C::Error>> {
        self.sensor
            .set_operation_mode(OperationMode::Forced)
            .await?;
        Timer::after_micros(u64::from(
            Bme68x::<I2cInterface<I2C>, Delay>::measurement_duration(
                OperationMode::Forced,
                &CONFIGURATION,
            ),
        ))
        .await;

        let measurements = self.sensor.measurements(OperationMode::Forced).await?;
        let measurement = measurements
            .as_slice()
            .first()
            .ok_or(EnvironmentError::NoData)?;
        Ok(EnvironmentReading {
            temperature_centidegrees: measurement.values.temperature,
            humidity_millipercent: measurement.values.humidity.min(100_000),
        })
    }
}
