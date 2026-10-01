//! Host-testable Sensirion SHTC3 temperature and humidity driver.
//!
//! Command values, timings, and the CRC parameters are taken from Waveshare's
//! ESP-IDF example for the ESP32-S3-RLCD-4.2, which is the authoritative source
//! for the part as wired on that board.
//!
//! Readings are returned **uncorrected**. Waveshare's own code subtracts a
//! fixed 4 °C for self-heating, but that is a property of where the sensor sits
//! relative to warm components on one board, not of the sensor. Boards apply
//! their own offset; a driver that baked one in would be silently wrong
//! everywhere else.

#![cfg_attr(not(test), no_std)]

use embedded_hal_async::i2c::I2c;

/// 7-bit I2C address. Fixed on this part.
pub const I2C_ADDRESS: u8 = 0x70;

const CMD_READ_ID: u16 = 0xEFC8;
const CMD_SOFT_RESET: u16 = 0x805D;
const CMD_SLEEP: u16 = 0xB098;
const CMD_WAKEUP: u16 = 0x3517;
/// Read temperature first, clock stretching disabled, so the bus is never held.
const CMD_MEASURE: u16 = 0x7866;

/// CRC-8, polynomial 0x31, initialised to 0xFF. Sensirion's usual parameters.
const CRC_POLYNOMIAL: u8 = 0x31;
const CRC_INIT: u8 = 0xFF;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<E> {
    Bus(E),
    /// A word's checksum did not match; the reading was discarded rather than
    /// reported, because a corrupt humidity looks entirely plausible.
    Checksum,
}

/// One measurement, in fixed point to avoid floats on the device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Measurement {
    /// Thousandths of a degree Celsius, uncorrected for board self-heating.
    pub temperature_millicelsius: i32,
    /// Thousandths of a percent relative humidity.
    pub humidity_millipercent: i32,
}

/// `T = -45 + 175 * raw / 2^16`, in millidegrees.
pub const fn temperature_millicelsius(raw: u16) -> i32 {
    ((175_000i64 * raw as i64) >> 16) as i32 - 45_000
}

/// `RH = 100 * raw / 2^16`, in milli-percent.
pub const fn humidity_millipercent(raw: u16) -> i32 {
    ((100_000i64 * raw as i64) >> 16) as i32
}

pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = CRC_INIT;
    for byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ CRC_POLYNOMIAL
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub struct Shtc3<I2C> {
    i2c: I2C,
}

impl<I2C: I2c> Shtc3<I2C> {
    pub const fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    async fn command(&mut self, command: u16) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(I2C_ADDRESS, &command.to_be_bytes())
            .await
            .map_err(Error::Bus)
    }

    /// Leave sleep. The part ignores everything else until this lands, so every
    /// exchange starts here.
    pub async fn wakeup(&mut self) -> Result<(), Error<I2C::Error>> {
        self.command(CMD_WAKEUP).await
    }

    pub async fn sleep(&mut self) -> Result<(), Error<I2C::Error>> {
        self.command(CMD_SLEEP).await
    }

    pub async fn soft_reset(&mut self) -> Result<(), Error<I2C::Error>> {
        self.command(CMD_SOFT_RESET).await
    }

    /// Device ID. Useful as a presence check that proves more than an ACK does.
    pub async fn read_id(&mut self) -> Result<u16, Error<I2C::Error>> {
        self.command(CMD_READ_ID).await?;
        let mut word = [0u8; 3];
        self.i2c
            .read(I2C_ADDRESS, &mut word)
            .await
            .map_err(Error::Bus)?;
        if crc8(&word[..2]) != word[2] {
            return Err(Error::Checksum);
        }
        Ok(u16::from_be_bytes([word[0], word[1]]))
    }

    /// Start a conversion. The caller waits before [`Self::read_measurement`],
    /// because this driver does not own a timer — 20ms is what Waveshare allows.
    pub async fn start_measurement(&mut self) -> Result<(), Error<I2C::Error>> {
        self.command(CMD_MEASURE).await
    }

    /// Collect a started conversion: temperature word, humidity word, each
    /// followed by its own CRC.
    pub async fn read_measurement(&mut self) -> Result<Measurement, Error<I2C::Error>> {
        let mut bytes = [0u8; 6];
        self.i2c
            .read(I2C_ADDRESS, &mut bytes)
            .await
            .map_err(Error::Bus)?;
        if crc8(&bytes[0..2]) != bytes[2] || crc8(&bytes[3..5]) != bytes[5] {
            return Err(Error::Checksum);
        }
        Ok(Measurement {
            temperature_millicelsius: temperature_millicelsius(u16::from_be_bytes([
                bytes[0], bytes[1],
            ])),
            humidity_millipercent: humidity_millipercent(u16::from_be_bytes([bytes[3], bytes[4]])),
        })
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    /// Sensirion's documented check value for this CRC's parameters.
    #[test]
    fn crc_matches_the_datasheet_check_value() {
        assert_eq!(crc8(&[0xBE, 0xEF]), 0x92);
    }

    #[test]
    fn crc_of_zero_word_is_stable() {
        assert_eq!(crc8(&[0x00, 0x00]), 0x81);
    }

    #[test]
    fn temperature_spans_the_sensor_range() {
        // raw 0 is the bottom of the transfer function, raw 0xFFFF the top.
        assert_eq!(temperature_millicelsius(0), -45_000);
        assert_eq!(temperature_millicelsius(u16::MAX), 129_997);
        // Mid-scale is the midpoint of -45..130.
        assert_eq!(temperature_millicelsius(0x8000), 42_500);
    }

    #[test]
    fn humidity_spans_zero_to_one_hundred() {
        assert_eq!(humidity_millipercent(0), 0);
        assert_eq!(humidity_millipercent(u16::MAX), 99_998);
        assert_eq!(humidity_millipercent(0x8000), 50_000);
    }

    #[test]
    fn a_plausible_room_reading_decodes() {
        // Raw words derived from the transfer functions, not guessed: 0x62E4
        // is the code for ~22.6 C and 0x7333 for ~45% RH.
        assert_eq!(temperature_millicelsius(0x62E4), 22_601);
        assert_eq!(humidity_millipercent(0x7333), 44_999);
    }
}
