//! PCF85063A register map and bit masks.
//!
//! Register-level authority: the PCF85063A datasheet
//! <https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf>.

/// 7-bit I2C address of the PCF85063A.
pub const I2C_ADDRESS: u8 = 0x51;

/// Control_1: mode/reset control.
pub const CONTROL_1: u8 = 0x00;
/// Control_2: interrupt/flag control (untouched by this driver).
pub const CONTROL_2: u8 = 0x01;
/// Offset: crystal aging/temperature calibration register.
///
/// This is the RTC's own calibration register, unrelated to the
/// application-level local UTC offset this driver stores in [`RAM_BYTE`].
/// This driver never reads or writes it.
pub const OFFSET_CALIBRATION: u8 = 0x02;
/// RAM_byte: the PCF85063A's one byte of undedicated free RAM. This driver
/// uses it exclusively to store the encoded fixed local offset.
pub const RAM_BYTE: u8 = 0x03;
pub const SECONDS: u8 = 0x04;
pub const MINUTES: u8 = 0x05;
pub const HOURS: u8 = 0x06;
pub const DAYS: u8 = 0x07;
pub const WEEKDAYS: u8 = 0x08;
pub const MONTHS: u8 = 0x09;
pub const YEARS: u8 = 0x0A;

/// First register of the single-burst block this driver reads/writes as one
/// transaction: `Control_1..=Years`, 11 bytes.
pub const BLOCK_START: u8 = CONTROL_1;
/// Length in bytes of the `Control_1..=Years` burst block.
pub const BLOCK_LEN: usize = (YEARS - CONTROL_1 + 1) as usize;

/// First register of the calendar-only sub-block this driver writes as its
/// own transaction during `TIMESET`: `Seconds..=Years`, 7 bytes.
pub const CALENDAR_START: u8 = SECONDS;
/// Length in bytes of the `Seconds..=Years` calendar sub-block.
pub const CALENDAR_LEN: usize = (YEARS - SECONDS + 1) as usize;

/// Offset within a `BLOCK_START`-based burst buffer of each register.
pub const BLOCK_OFFSET_CONTROL_1: usize = (CONTROL_1 - BLOCK_START) as usize;
pub const BLOCK_OFFSET_RAM_BYTE: usize = (RAM_BYTE - BLOCK_START) as usize;
pub const BLOCK_OFFSET_SECONDS: usize = (SECONDS - BLOCK_START) as usize;
pub const BLOCK_OFFSET_MINUTES: usize = (MINUTES - BLOCK_START) as usize;
pub const BLOCK_OFFSET_HOURS: usize = (HOURS - BLOCK_START) as usize;
pub const BLOCK_OFFSET_DAYS: usize = (DAYS - BLOCK_START) as usize;
pub const BLOCK_OFFSET_WEEKDAYS: usize = (WEEKDAYS - BLOCK_START) as usize;
pub const BLOCK_OFFSET_MONTHS: usize = (MONTHS - BLOCK_START) as usize;
pub const BLOCK_OFFSET_YEARS: usize = (YEARS - BLOCK_START) as usize;

/// Offset within a `CALENDAR_START`-based buffer of each calendar register.
pub const CALENDAR_OFFSET_SECONDS: usize = (SECONDS - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_MINUTES: usize = (MINUTES - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_HOURS: usize = (HOURS - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_DAYS: usize = (DAYS - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_WEEKDAYS: usize = (WEEKDAYS - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_MONTHS: usize = (MONTHS - CALENDAR_START) as usize;
pub const CALENDAR_OFFSET_YEARS: usize = (YEARS - CALENDAR_START) as usize;

/// Control_1 bit masks.
pub mod control_1 {
    /// External clock test mode. Must always be zero in normal operation.
    pub const EXT_TEST: u8 = 0x80;
    /// Bit 6 is unused/reserved; must always be written zero.
    pub const UNUSED_BIT6: u8 = 0x40;
    /// Stop bit: 1 halts the RTC source clock (register access remains
    /// possible), 0 runs it.
    pub const STOP: u8 = 0x20;
    /// Software-reset trigger. Must always be zero on ordinary writes; this
    /// driver never issues a software reset.
    pub const SR: u8 = 0x10;
    /// Bit 3 is unused/reserved; must always be written zero.
    pub const UNUSED_BIT3: u8 = 0x08;
    /// Correction interrupt enable. Preserved verbatim by `TIMESET`.
    pub const CIE: u8 = 0x04;
    /// Hour mode: 0 = 24-hour (canonical), 1 = 12-hour. Forced to zero by
    /// `TIMESET`.
    pub const HOUR_MODE_12: u8 = 0x02;
    /// Oscillator capacitor selection. Preserved verbatim by `TIMESET`.
    pub const CAP_SEL: u8 = 0x01;

    /// Bits `TIMESET` preserves from the existing Control_1 value.
    pub const PRESERVED_MASK: u8 = CIE | CAP_SEL;
    /// Bits `TIMESET` always forces to zero (in addition to `STOP`, which is
    /// driven explicitly by the assert/release steps).
    pub const FORCED_ZERO_MASK: u8 = EXT_TEST | UNUSED_BIT6 | SR | UNUSED_BIT3 | HOUR_MODE_12;
}

/// Seconds register bit masks.
pub mod seconds {
    /// Oscillator-stop flag: set when the oscillator has stopped (e.g. after
    /// total power loss) and calendar data is no longer reliable, until
    /// explicitly cleared by a calendar write.
    pub const OS: u8 = 0x80;
    /// BCD seconds value mask (0..=59).
    pub const VALUE_MASK: u8 = 0x7F;
}

pub mod minutes {
    /// BCD minutes value mask (0..=59). Bit 7 is unused/reserved.
    pub const VALUE_MASK: u8 = 0x7F;
}

pub mod hours_24h {
    /// BCD hours value mask in 24-hour mode (0..=23).
    pub const VALUE_MASK: u8 = 0x3F;
}

pub mod days {
    /// BCD day-of-month value mask (1..=31).
    pub const VALUE_MASK: u8 = 0x3F;
}

pub mod weekdays {
    /// Weekday value mask (0..=6).
    pub const VALUE_MASK: u8 = 0x07;
}

pub mod months {
    /// BCD month value mask (1..=12).
    pub const VALUE_MASK: u8 = 0x1F;
}
