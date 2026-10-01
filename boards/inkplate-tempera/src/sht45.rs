//! Optional external Sensirion SHT45 on the Inkplate easyC/Qwiic bus.
//!
//! The breakout is not part of the board, so product code decides whether a
//! failed read means "not installed" or a degraded optional source. This
//! module only implements one high-precision, no-heater conversion at the
//! SHT4x family's fixed I2C address.

use embassy_time::Timer;
use embedded_hal_async::i2c::I2c;

use crate::environment::EnvironmentReading;

pub const SHT45_ADDRESS: u8 = 0x44;
const MEASURE_HIGH_PRECISION: u8 = 0xFD;
const MEASUREMENT_TIME_MS: u64 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sht45Word {
    Temperature,
    Humidity,
}

#[derive(Debug)]
pub enum Sht45Error<E> {
    I2c(E),
    Crc(Sht45Word),
}

/// A SHT45 connected to either of the Inkplate's electrically identical
/// easyC/Qwiic chain positions.
pub struct Sht45<I2C> {
    i2c: I2C,
}

impl<I2C> Sht45<I2C>
where
    I2C: I2c,
{
    pub const fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    /// Run a high-precision conversion without enabling the sensor heater.
    pub async fn sample(&mut self) -> Result<EnvironmentReading, Sht45Error<I2C::Error>> {
        self.i2c
            .write(SHT45_ADDRESS, &[MEASURE_HIGH_PRECISION])
            .await
            .map_err(Sht45Error::I2c)?;
        Timer::after_millis(MEASUREMENT_TIME_MS).await;

        let mut response = [0_u8; 6];
        self.i2c
            .read(SHT45_ADDRESS, &mut response)
            .await
            .map_err(Sht45Error::I2c)?;
        decode_response(response)
    }
}

fn decode_response<E>(response: [u8; 6]) -> Result<EnvironmentReading, Sht45Error<E>> {
    if crc8(&response[0..2]) != response[2] {
        return Err(Sht45Error::Crc(Sht45Word::Temperature));
    }
    if crc8(&response[3..5]) != response[5] {
        return Err(Sht45Error::Crc(Sht45Word::Humidity));
    }

    let raw_temperature = u16::from_be_bytes([response[0], response[1]]);
    let raw_humidity = u16::from_be_bytes([response[3], response[4]]);
    Ok(EnvironmentReading {
        temperature_centidegrees: temperature_centidegrees(raw_temperature),
        humidity_millipercent: humidity_millipercent(raw_humidity),
    })
}

fn temperature_centidegrees(raw: u16) -> i16 {
    let scaled = (17_500_u32 * u32::from(raw) + 32_767) / 65_535;
    (scaled as i32 - 4_500) as i16
}

fn humidity_millipercent(raw: u16) -> u32 {
    let scaled = (125_000_u64 * u64::from(raw) + 32_767) / 65_535;
    (scaled as i64 - 6_000).clamp(0, 100_000) as u32
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0xFF_u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x31
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_sensirion_reference_vector() {
        assert_eq!(crc8(&[0xBE, 0xEF]), 0x92);
    }

    #[test]
    fn conversion_clamps_physical_humidity_range() {
        assert_eq!(temperature_centidegrees(0), -4_500);
        assert_eq!(temperature_centidegrees(u16::MAX), 13_000);
        assert_eq!(humidity_millipercent(0), 0);
        assert_eq!(humidity_millipercent(u16::MAX), 100_000);
    }

    #[test]
    fn response_rejects_each_corrupt_word() {
        let mut response = [0x66, 0x66, 0, 0x66, 0x66, 0];
        response[2] = crc8(&response[0..2]);
        response[5] = crc8(&response[3..5]);
        assert!(decode_response::<()>(response).is_ok());

        response[2] ^= 1;
        assert!(matches!(
            decode_response::<()>(response),
            Err(Sht45Error::Crc(Sht45Word::Temperature))
        ));
        response[2] ^= 1;
        response[5] ^= 1;
        assert!(matches!(
            decode_response::<()>(response),
            Err(Sht45Error::Crc(Sht45Word::Humidity))
        ));
    }
}
