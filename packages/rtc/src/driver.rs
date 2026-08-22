//! PCF85063A hardware driver: fresh reads (`read_snapshot`) and the
//! failure-safe `TIMESET` write sequence (`time_set`).
//!
//! Generic over any `embedded_hal_async::i2c::I2c` implementation, so it
//! runs unmodified on the ESP32 target (over a shared-bus `I2cDevice`) and
//! against a fake bus in host tests.

use embedded_hal_async::i2c::I2c;

use crate::{
    calendar::{self, Calendar},
    offset,
    registers::{self, control_1, days, hours_24h, minutes, months, seconds, weekdays},
};

/// One register/transaction beyond the largest block this driver writes in
/// a single transaction (the 11-byte `Control_1..=Years` block, plus its
/// leading register-address byte).
const MAX_TRANSACTION_LEN: usize = registers::BLOCK_LEN + 1;

/// Stable failure reasons for `TIMESET`, matching the serial wire protocol's
/// `TIMESET ERR reason=<..>` vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RtcError<E> {
    /// `utc_epoch_seconds` does not fall within the representable
    /// 2000-01-01..=2099-12-31 window.
    Range,
    /// `offset_minutes` is outside UTC-12:00..=UTC+14:00 or not a multiple
    /// of 15 minutes.
    Offset,
    /// An I2C transaction failed.
    I2c(E),
    /// The post-write readback did not match the requested calendar/offset.
    Verify,
    /// The post-write readback found `STOP` or the oscillator-stop flag
    /// asserted: the clock is not actually running.
    ClockStopped,
}

impl<E> RtcError<E> {
    /// Stable snake_case reason string for the serial wire protocol.
    pub const fn label(&self) -> &'static str {
        match self {
            RtcError::Range => "range",
            RtcError::Offset => "offset",
            RtcError::I2c(_) => "i2c",
            RtcError::Verify => "verify",
            RtcError::ClockStopped => "clock_stopped",
        }
    }
}

/// Why [`Pcf85063a::read_snapshot`] considers wall-clock time unavailable,
/// matching `TIMEGET OK valid=off reason=<..>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnavailableReason {
    OscillatorStopped,
    ClockStopped,
    OffsetUnset,
    InvalidCalendar,
}

impl UnavailableReason {
    /// Stable snake_case reason string for the serial wire protocol.
    pub const fn label(&self) -> &'static str {
        match self {
            UnavailableReason::OscillatorStopped => "oscillator_stopped",
            UnavailableReason::ClockStopped => "clock_stopped",
            UnavailableReason::OffsetUnset => "offset_unset",
            UnavailableReason::InvalidCalendar => "invalid_calendar",
        }
    }
}

/// A fresh `TIMEGET` read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WallClockSnapshot {
    pub valid: bool,
    /// Meaningless (always `0`) when `valid` is `false`; use `reason` instead.
    pub utc_epoch_seconds: u32,
    /// Meaningless (always `0`) when `valid` is `false`.
    pub local_epoch_seconds: u32,
    /// Meaningless (always `0`) when `valid` is `false`.
    pub offset_minutes: i16,
    /// Set only when `valid` is `false`.
    pub reason: Option<UnavailableReason>,
}

impl WallClockSnapshot {
    const fn unavailable(reason: UnavailableReason) -> Self {
        Self {
            valid: false,
            utc_epoch_seconds: 0,
            local_epoch_seconds: 0,
            offset_minutes: 0,
            reason: Some(reason),
        }
    }
}

/// A successful `TIMESET` readback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeSetOutcome {
    pub utc_epoch_seconds: u32,
    pub offset_minutes: i16,
}

/// PCF85063A driver over a shared I2C bus. Owns no cache: every call is a
/// fresh transaction against the device.
pub struct Pcf85063a<I2C> {
    i2c: I2C,
}

impl<I2C> Pcf85063a<I2C>
where
    I2C: I2c,
{
    pub const fn new(i2c: I2C) -> Self {
        Self { i2c }
    }

    /// Reads the `Control_1..=Years` block in one burst and interprets it.
    ///
    /// Precedence when unavailable (checked in this order): an asserted
    /// `STOP` bit is treated as a stopped clock even if the oscillator is
    /// otherwise running; then the oscillator-stop flag; then an
    /// undecodable/out-of-range calendar or an inconsistent weekday; then an
    /// unset/invalid offset marker.
    pub async fn read_snapshot(&mut self) -> Result<WallClockSnapshot, RtcError<I2C::Error>> {
        let mut block = [0u8; registers::BLOCK_LEN];
        self.read_bytes(registers::BLOCK_START, &mut block).await?;

        let stop_bit = block[registers::BLOCK_OFFSET_CONTROL_1] & control_1::STOP != 0;
        let oscillator_stopped = block[registers::BLOCK_OFFSET_SECONDS] & seconds::OS != 0;
        let decoded_calendar = decode_calendar_from_block(&block);
        let weekday_consistent = decoded_calendar.is_some_and(|calendar| {
            calendar.weekday() == block[registers::BLOCK_OFFSET_WEEKDAYS] & weekdays::VALUE_MASK
        });
        let decoded_offset = offset::decode(block[registers::BLOCK_OFFSET_RAM_BYTE]);

        if stop_bit {
            return Ok(WallClockSnapshot::unavailable(
                UnavailableReason::ClockStopped,
            ));
        }
        if oscillator_stopped {
            return Ok(WallClockSnapshot::unavailable(
                UnavailableReason::OscillatorStopped,
            ));
        }
        let Some(calendar) = decoded_calendar.filter(|_| weekday_consistent) else {
            return Ok(WallClockSnapshot::unavailable(
                UnavailableReason::InvalidCalendar,
            ));
        };
        let Some(offset_minutes) = decoded_offset else {
            return Ok(WallClockSnapshot::unavailable(
                UnavailableReason::OffsetUnset,
            ));
        };

        let utc_epoch_seconds = calendar.to_epoch_seconds().ok_or(RtcError::Verify)?;
        Ok(WallClockSnapshot {
            valid: true,
            utc_epoch_seconds,
            local_epoch_seconds: apply_offset(utc_epoch_seconds, offset_minutes),
            offset_minutes,
            reason: None,
        })
    }

    /// Failure-safe `TIMESET`: validates input with no bus I/O, invalidates
    /// the offset marker, asserts `STOP`, writes the calendar, releases
    /// `STOP`, writes the offset, then reads back and verifies. Every
    /// failure path leaves the offset marker unset and best-effort releases
    /// `STOP`; no software reset is ever issued.
    pub async fn time_set(
        &mut self,
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    ) -> Result<TimeSetOutcome, RtcError<I2C::Error>> {
        let calendar = Calendar::from_epoch_seconds(utc_epoch_seconds).ok_or(RtcError::Range)?;
        let encoded_offset = offset::encode(offset_minutes).ok_or(RtcError::Offset)?;

        // Fail-safe ordering: invalidate the offset marker before touching
        // the calendar, so a crash mid-sequence never leaves a stale-but-
        // plausible offset paired with an unrelated calendar.
        self.write_register(registers::RAM_BYTE, offset::UNSET)
            .await?;

        let control_1 = self.read_register(registers::CONTROL_1).await?;
        let normalized = control_1 & control_1::PRESERVED_MASK;

        if let Err(error) = self
            .write_register(registers::CONTROL_1, normalized | control_1::STOP)
            .await
        {
            let _ = self.write_register(registers::CONTROL_1, normalized).await;
            return Err(error);
        }

        let calendar_bytes = encode_calendar_block(&calendar);
        if let Err(error) = self
            .write_bytes(registers::CALENDAR_START, &calendar_bytes)
            .await
        {
            let _ = self.write_register(registers::CONTROL_1, normalized).await;
            return Err(error);
        }

        // Release STOP with the same normalized Control_1 value used to
        // assert it.
        self.write_register(registers::CONTROL_1, normalized)
            .await?;
        self.write_register(registers::RAM_BYTE, encoded_offset)
            .await?;

        let mut block = [0u8; registers::BLOCK_LEN];
        self.read_bytes(registers::BLOCK_START, &mut block).await?;

        let readback_stop = block[registers::BLOCK_OFFSET_CONTROL_1] & control_1::STOP != 0;
        let readback_os = block[registers::BLOCK_OFFSET_SECONDS] & seconds::OS != 0;
        if readback_stop || readback_os {
            return Err(RtcError::ClockStopped);
        }

        let readback_calendar = decode_calendar_from_block(&block).ok_or(RtcError::Verify)?;
        let readback_weekday = block[registers::BLOCK_OFFSET_WEEKDAYS] & weekdays::VALUE_MASK;
        let readback_offset = offset::decode(block[registers::BLOCK_OFFSET_RAM_BYTE]);
        if readback_calendar != calendar
            || readback_weekday != calendar.weekday()
            || readback_offset != Some(offset_minutes)
        {
            return Err(RtcError::Verify);
        }

        let readback_utc = readback_calendar
            .to_epoch_seconds()
            .ok_or(RtcError::Verify)?;
        Ok(TimeSetOutcome {
            utc_epoch_seconds: readback_utc,
            offset_minutes,
        })
    }

    async fn read_bytes(&mut self, start: u8, buf: &mut [u8]) -> Result<(), RtcError<I2C::Error>> {
        self.i2c
            .write_read(registers::I2C_ADDRESS, &[start], buf)
            .await
            .map_err(RtcError::I2c)
    }

    async fn write_bytes(&mut self, start: u8, data: &[u8]) -> Result<(), RtcError<I2C::Error>> {
        debug_assert!(data.len() < MAX_TRANSACTION_LEN);
        let mut scratch = [0u8; MAX_TRANSACTION_LEN];
        scratch[0] = start;
        scratch[1..1 + data.len()].copy_from_slice(data);
        self.i2c
            .write(registers::I2C_ADDRESS, &scratch[..1 + data.len()])
            .await
            .map_err(RtcError::I2c)
    }

    async fn read_register(&mut self, reg: u8) -> Result<u8, RtcError<I2C::Error>> {
        let mut buf = [0u8; 1];
        self.read_bytes(reg, &mut buf).await?;
        Ok(buf[0])
    }

    async fn write_register(&mut self, reg: u8, value: u8) -> Result<(), RtcError<I2C::Error>> {
        self.write_bytes(reg, &[value]).await
    }
}

/// Adds a fixed offset in minutes to a UTC epoch. Safe for every
/// representable `(utc_epoch_seconds, offset_minutes)` pair produced by this
/// driver: `utc_epoch_seconds` is always within 2000-2099 and
/// `offset_minutes` is bounded to +/-a few hundred minutes, so the sum
/// never approaches `u32`'s range limits.
fn apply_offset(utc_epoch_seconds: u32, offset_minutes: i16) -> u32 {
    (utc_epoch_seconds as i64 + offset_minutes as i64 * 60) as u32
}

/// Encodes a validated [`Calendar`] into the 7-byte `Seconds..=Years` block,
/// with the oscillator-stop flag (seconds bit 7) left clear.
fn encode_calendar_block(calendar: &Calendar) -> [u8; registers::CALENDAR_LEN] {
    let mut bytes = [0u8; registers::CALENDAR_LEN];
    bytes[registers::CALENDAR_OFFSET_SECONDS] = bcd(calendar.second);
    bytes[registers::CALENDAR_OFFSET_MINUTES] = bcd(calendar.minute);
    bytes[registers::CALENDAR_OFFSET_HOURS] = bcd(calendar.hour);
    bytes[registers::CALENDAR_OFFSET_DAYS] = bcd(calendar.day);
    bytes[registers::CALENDAR_OFFSET_WEEKDAYS] = calendar.weekday();
    bytes[registers::CALENDAR_OFFSET_MONTHS] = bcd(calendar.month);
    bytes[registers::CALENDAR_OFFSET_YEARS] = bcd((calendar.year - calendar::MIN_YEAR) as u8);
    bytes
}

/// BCD-encodes a calendar field already guaranteed `< 100` by
/// [`Calendar::new`]'s validation (seconds/minutes <= 59, hours <= 23,
/// days <= 31, months <= 12, year offset <= 99).
fn bcd(value: u8) -> u8 {
    calendar::bcd_encode(value).expect("calendar fields are always < 100 by construction")
}

/// Decodes the calendar portion of an 11-byte `Control_1..=Years` block.
/// Returns `None` if any field is not valid BCD or the resulting date is
/// out of range.
fn decode_calendar_from_block(block: &[u8; registers::BLOCK_LEN]) -> Option<Calendar> {
    let second =
        calendar::bcd_decode(block[registers::BLOCK_OFFSET_SECONDS] & seconds::VALUE_MASK)?;
    let minute =
        calendar::bcd_decode(block[registers::BLOCK_OFFSET_MINUTES] & minutes::VALUE_MASK)?;
    let hour = calendar::bcd_decode(block[registers::BLOCK_OFFSET_HOURS] & hours_24h::VALUE_MASK)?;
    let day = calendar::bcd_decode(block[registers::BLOCK_OFFSET_DAYS] & days::VALUE_MASK)?;
    let month = calendar::bcd_decode(block[registers::BLOCK_OFFSET_MONTHS] & months::VALUE_MASK)?;
    let year_two_digit = calendar::bcd_decode(block[registers::BLOCK_OFFSET_YEARS])?;
    let year = calendar::MIN_YEAR + u16::from(year_two_digit);
    Calendar::new(year, month, day, hour, minute, second)
}
