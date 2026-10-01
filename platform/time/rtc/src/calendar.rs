//! Gregorian calendar <-> Unix epoch conversion, BCD encoding, and weekday
//! computation for the 2000-2099 window the PCF85063A's two-digit BCD year
//! register can represent.
//!
//! The day-count math is Howard Hinnant's `days_from_civil`/`civil_from_days`
//! algorithm (<http://howardhinnant.github.io/date_algorithms.html>), a
//! standard proleptic-Gregorian day-count conversion valid over a far wider
//! range than we need here; correctness at the 2000/2099 boundaries and
//! every leap year in between follows from it holding for all years.

/// Earliest calendar year the RTC's two-digit BCD year register can encode.
pub const MIN_YEAR: u16 = 2000;
/// Latest calendar year the RTC's two-digit BCD year register can encode.
pub const MAX_YEAR: u16 = 2099;

/// A validated Gregorian date and time of day within [`MIN_YEAR`]..=[`MAX_YEAR`].
///
/// The weekday is never stored: it is always derived from `year`/`month`/`day`
/// via [`Calendar::weekday`], so a `Calendar` can never hold a date and
/// weekday that disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Calendar {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Calendar {
    /// Validates and constructs a calendar date/time, or returns `None` if
    /// any field is out of range (including a day that does not exist in the
    /// given month/year, and years outside the RTC's representable window).
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> Option<Self> {
        if !is_valid_calendar(year, month, day, hour, minute, second) {
            return None;
        }
        Some(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        })
    }

    /// Day of week: `0` = Sunday .. `6` = Saturday.
    pub fn weekday(&self) -> u8 {
        weekday_of(self.year, self.month, self.day)
    }

    /// Converts to Unix epoch seconds. Returns `None` only if the caller
    /// somehow built a `Calendar` bypassing [`Calendar::new`]'s validation
    /// (the public constructor already guarantees a representable value).
    pub fn to_epoch_seconds(&self) -> Option<u32> {
        if !is_valid_calendar(
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
        ) {
            return None;
        }
        let days = days_from_civil(self.year as i64, self.month as u32, self.day as u32);
        let seconds = days
            .checked_mul(86_400)?
            .checked_add(self.hour as i64 * 3_600)?
            .checked_add(self.minute as i64 * 60)?
            .checked_add(self.second as i64)?;
        u32::try_from(seconds).ok()
    }

    /// Converts Unix epoch seconds to a calendar date/time. Returns `None`
    /// when the resulting year falls outside [`MIN_YEAR`]..=[`MAX_YEAR`].
    pub fn from_epoch_seconds(epoch_seconds: u32) -> Option<Self> {
        let epoch_seconds = epoch_seconds as i64;
        let days = epoch_seconds.div_euclid(86_400);
        let time_of_day = epoch_seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        if !(i64::from(MIN_YEAR)..=i64::from(MAX_YEAR)).contains(&year) {
            return None;
        }
        let hour = (time_of_day / 3_600) as u8;
        let minute = ((time_of_day % 3_600) / 60) as u8;
        let second = (time_of_day % 60) as u8;
        Calendar::new(year as u16, month as u8, day as u8, hour, minute, second)
    }
}

/// Whether `year` is a Gregorian leap year.
pub const fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Number of days in `month` of `year` (`month` is 1..=12), or `None` if
/// `month` is out of range.
pub const fn days_in_month(year: u16, month: u8) -> Option<u8> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 => Some(if is_leap_year(year) { 29 } else { 28 }),
        _ => None,
    }
}

/// Whether the given fields form a representable, in-range calendar
/// date/time: year in [`MIN_YEAR`]..=[`MAX_YEAR`], month 1..=12, a day that
/// exists in that month/year, hour 0..=23, minute/second 0..=59.
pub const fn is_valid_calendar(
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> bool {
    if year < MIN_YEAR || year > MAX_YEAR {
        return false;
    }
    let max_day = match days_in_month(year, month) {
        Some(max_day) => max_day,
        None => return false,
    };
    if day == 0 || day > max_day {
        return false;
    }
    hour <= 23 && minute <= 59 && second <= 59
}

/// Day of week for a valid Gregorian date: `0` = Sunday .. `6` = Saturday.
/// Callers must pass a date that already passes [`is_valid_calendar`];
/// invalid input still returns a value (this function does not fail) but
/// that value is meaningless.
pub fn weekday_of(year: u16, month: u8, day: u8) -> u8 {
    let days = days_from_civil(year as i64, month as u32, day as u32);
    // 1970-01-01 (days == 0) was a Thursday, index 4 under 0 == Sunday.
    ((days.rem_euclid(7)) + 4).rem_euclid(7) as u8
}

/// Encodes a value 0..=99 as a single BCD byte. Returns `None` if `value`
/// exceeds 99 (cannot be represented in two BCD nibbles).
pub const fn bcd_encode(value: u8) -> Option<u8> {
    if value > 99 {
        return None;
    }
    Some(((value / 10) << 4) | (value % 10))
}

/// Decodes a BCD byte to its integer value. Returns `None` if either nibble
/// is not a valid decimal digit (0..=9) — i.e. the byte is not valid BCD.
pub const fn bcd_decode(byte: u8) -> Option<u8> {
    let high = byte >> 4;
    let low = byte & 0x0F;
    if high > 9 || low > 9 {
        return None;
    }
    Some(high * 10 + low)
}

/// Days since 1970-01-01 for a proleptic-Gregorian civil date. `month` is
/// 1..=12, `day` is 1..=31 (callers are expected to have already validated
/// the date; out-of-range `day`/`month` still produce a definite, if
/// meaningless, result rather than panicking).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = (if year >= 0 { year } else { year - 399 }) / 400;
    let year_of_era = year - era * 400;
    let month_index = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Inverse of [`days_from_civil`]: proleptic-Gregorian `(year, month, day)`
/// for the given day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}
