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

/// Which driver transaction an I2C failure belongs to. Reported to the
/// diagnostic sink; never affects the returned [`RtcError`] or its label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RtcStage {
    SnapshotRead,
    InvalidateMarker,
    ReadControl,
    AssertStop,
    WriteCalendar,
    ReleaseStop,
    WriteOffset,
    ImmediateReadback,
    FailureInvalidate,
}

impl RtcStage {
    /// Stable snake_case name for diagnostic lines.
    pub const fn as_str(&self) -> &'static str {
        match self {
            RtcStage::SnapshotRead => "snapshot_read",
            RtcStage::InvalidateMarker => "invalidate_marker",
            RtcStage::ReadControl => "read_control",
            RtcStage::AssertStop => "assert_stop",
            RtcStage::WriteCalendar => "write_calendar",
            RtcStage::ReleaseStop => "release_stop",
            RtcStage::WriteOffset => "write_offset",
            RtcStage::ImmediateReadback => "immediate_readback",
            RtcStage::FailureInvalidate => "failure_invalidate",
        }
    }
}

/// Receives the original I2C error by reference on every failed driver
/// transaction, with the stage, register, and device address attached.
/// Failure-only: never called on success, and never called with
/// payload bytes or time values.
pub trait RtcDiagnosticSink {
    fn on_i2c_error<E>(&mut self, stage: RtcStage, register: u8, address: u8, error: &E)
    where
        E: core::fmt::Debug;
}

/// Default sink: discards every diagnostic.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoDiagnostics;

impl RtcDiagnosticSink for NoDiagnostics {
    fn on_i2c_error<E>(&mut self, _stage: RtcStage, _register: u8, _address: u8, _error: &E)
    where
        E: core::fmt::Debug,
    {
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
/// fresh transaction against the device. The sink only observes failed
/// transactions; it does not replace driver transactions or returned errors.
pub struct Pcf85063a<I2C, D = NoDiagnostics> {
    i2c: I2C,
    sink: D,
}

impl<I2C> Pcf85063a<I2C, NoDiagnostics>
where
    I2C: I2c,
{
    pub const fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            sink: NoDiagnostics,
        }
    }
}

impl<I2C, D> Pcf85063a<I2C, D>
where
    I2C: I2c,
    D: RtcDiagnosticSink,
{
    pub const fn with_diagnostics(i2c: I2C, sink: D) -> Self {
        Self { i2c, sink }
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
        self.read_bytes(RtcStage::SnapshotRead, registers::BLOCK_START, &mut block)
            .await?;

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
        self.write_register(
            RtcStage::InvalidateMarker,
            registers::RAM_BYTE,
            offset::UNSET,
        )
        .await?;

        let control_1 = self
            .read_register(RtcStage::ReadControl, registers::CONTROL_1)
            .await?;
        let normalized = control_1 & control_1::PRESERVED_MASK;

        if let Err(error) = self
            .write_register(
                RtcStage::AssertStop,
                registers::CONTROL_1,
                normalized | control_1::STOP,
            )
            .await
        {
            let _ = self
                .write_register(RtcStage::ReleaseStop, registers::CONTROL_1, normalized)
                .await;
            return Err(error);
        }

        let calendar_bytes = encode_calendar_block(&calendar);
        if let Err(error) = self
            .write_bytes(
                RtcStage::WriteCalendar,
                registers::CALENDAR_START,
                &calendar_bytes,
            )
            .await
        {
            let _ = self
                .write_register(RtcStage::ReleaseStop, registers::CONTROL_1, normalized)
                .await;
            return Err(error);
        }

        // Release STOP with the same normalized Control_1 value used to
        // assert it.
        self.write_register(RtcStage::ReleaseStop, registers::CONTROL_1, normalized)
            .await?;
        if let Err(error) = self
            .write_register(RtcStage::WriteOffset, registers::RAM_BYTE, encoded_offset)
            .await
        {
            // The write itself failed -- best-effort re-invalidate in case it
            // partially landed on the wire despite the reported error, so a
            // stale-but-plausible offset can never survive next to whatever
            // calendar happens to be there.
            self.best_effort_invalidate_offset_marker().await;
            return Err(error);
        }

        // From here on, `encoded_offset` is live in RAM_BYTE and looks like a
        // valid, synchronized offset. Every failure path below must
        // best-effort re-invalidate it before returning -- otherwise a failed
        // readback or a verification mismatch would leave a plausible offset
        // marker paired with a calendar nobody has confirmed matches it.
        let mut block = [0u8; registers::BLOCK_LEN];
        if let Err(error) = self
            .read_bytes(
                RtcStage::ImmediateReadback,
                registers::BLOCK_START,
                &mut block,
            )
            .await
        {
            self.best_effort_invalidate_offset_marker().await;
            return Err(error);
        }

        let readback_stop = block[registers::BLOCK_OFFSET_CONTROL_1] & control_1::STOP != 0;
        let readback_os = block[registers::BLOCK_OFFSET_SECONDS] & seconds::OS != 0;
        if readback_stop || readback_os {
            self.best_effort_invalidate_offset_marker().await;
            return Err(RtcError::ClockStopped);
        }

        let readback_calendar = decode_calendar_from_block(&block);
        let readback_weekday = block[registers::BLOCK_OFFSET_WEEKDAYS] & weekdays::VALUE_MASK;
        let readback_offset = offset::decode(block[registers::BLOCK_OFFSET_RAM_BYTE]);
        let verified = readback_calendar.filter(|calendar_reading| {
            *calendar_reading == calendar
                && readback_weekday == calendar.weekday()
                && readback_offset == Some(offset_minutes)
        });
        let Some(readback_calendar) = verified else {
            self.best_effort_invalidate_offset_marker().await;
            return Err(RtcError::Verify);
        };

        let Some(readback_utc) = readback_calendar.to_epoch_seconds() else {
            self.best_effort_invalidate_offset_marker().await;
            return Err(RtcError::Verify);
        };
        Ok(TimeSetOutcome {
            utc_epoch_seconds: readback_utc,
            offset_minutes,
        })
    }

    /// Re-invalidates the offset marker, ignoring any further I2C failure --
    /// called only from failure paths that occur after the marker has
    /// already been written live, where there is no better recovery than
    /// "try once more, and accept whatever state results either way."
    async fn best_effort_invalidate_offset_marker(&mut self) {
        let _ = self
            .write_register(
                RtcStage::FailureInvalidate,
                registers::RAM_BYTE,
                offset::UNSET,
            )
            .await;
    }

    async fn read_bytes(
        &mut self,
        stage: RtcStage,
        start: u8,
        buf: &mut [u8],
    ) -> Result<(), RtcError<I2C::Error>> {
        match self
            .i2c
            .write_read(registers::I2C_ADDRESS, &[start], buf)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                self.sink
                    .on_i2c_error(stage, start, registers::I2C_ADDRESS, &error);
                Err(RtcError::I2c(error))
            }
        }
    }

    async fn write_bytes(
        &mut self,
        stage: RtcStage,
        start: u8,
        data: &[u8],
    ) -> Result<(), RtcError<I2C::Error>> {
        debug_assert!(data.len() < MAX_TRANSACTION_LEN);
        let mut scratch = [0u8; MAX_TRANSACTION_LEN];
        scratch[0] = start;
        scratch[1..1 + data.len()].copy_from_slice(data);
        match self
            .i2c
            .write(registers::I2C_ADDRESS, &scratch[..1 + data.len()])
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => {
                self.sink
                    .on_i2c_error(stage, start, registers::I2C_ADDRESS, &error);
                Err(RtcError::I2c(error))
            }
        }
    }

    async fn read_register(
        &mut self,
        stage: RtcStage,
        reg: u8,
    ) -> Result<u8, RtcError<I2C::Error>> {
        let mut buf = [0u8; 1];
        self.read_bytes(stage, reg, &mut buf).await?;
        Ok(buf[0])
    }

    async fn write_register(
        &mut self,
        stage: RtcStage,
        reg: u8,
        value: u8,
    ) -> Result<(), RtcError<I2C::Error>> {
        self.write_bytes(stage, reg, &[value]).await
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
