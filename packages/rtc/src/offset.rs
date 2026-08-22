//! Encoding of the fixed local UTC offset into the PCF85063A's one free RAM
//! byte ([`crate::registers::RAM_BYTE`]).
//!
//! Offsets are accepted from UTC-12:00 through UTC+14:00 in 15-minute steps.
//! The encoded byte is `offset_quarter_hours + ENCODE_BIAS`, chosen so the
//! valid range (`VALID_RANGE`) structurally excludes `0xAA` — the marker
//! byte written by prior Arduino PCF85063A libraries to mean "time was set,
//! ignore contents" — so that legacy marker can never be misread as a
//! plausible offset. [`UNSET`] is a second, distinct sentinel this driver
//! writes itself to mean "no fixed offset has been synchronized yet".
//! [`decode`] treats every byte outside `VALID_RANGE` — `UNSET`, the legacy
//! `0xAA` marker, and any other garbage — identically as "no valid offset".

/// Smallest representable offset, in 15-minute steps: UTC-12:00.
const OFFSET_QUARTER_MIN: i16 = -48;
/// Largest representable offset, in 15-minute steps: UTC+14:00.
const OFFSET_QUARTER_MAX: i16 = 56;
const MINUTES_PER_QUARTER: i16 = 15;
/// Bias applied to a quarter-hour count to obtain the stored byte. Chosen so
/// the resulting valid range (52..=156) excludes both `0xAA` (170) and
/// [`UNSET`] (255).
const ENCODE_BIAS: i16 = 100;

/// Smallest valid encoded byte (inclusive).
pub const VALID_RANGE_MIN: u8 = (OFFSET_QUARTER_MIN + ENCODE_BIAS) as u8;
/// Largest valid encoded byte (inclusive).
pub const VALID_RANGE_MAX: u8 = (OFFSET_QUARTER_MAX + ENCODE_BIAS) as u8;

/// Sentinel this driver writes to mark "offset not yet synchronized".
/// Distinct from, and outside the valid range alongside, the legacy
/// Arduino-library marker below.
pub const UNSET: u8 = 0xFF;

/// The marker byte written by prior Arduino PCF85063A libraries. Never
/// written by this driver; documented so callers understand why it is
/// deliberately excluded from `VALID_RANGE_MIN..=VALID_RANGE_MAX`.
pub const LEGACY_ARDUINO_MARKER: u8 = 0xAA;

const _: () =
    assert!(LEGACY_ARDUINO_MARKER < VALID_RANGE_MIN || LEGACY_ARDUINO_MARKER > VALID_RANGE_MAX);
// `UNSET` (0xFF) is `u8::MAX` by design -- always outside the valid range,
// however that range is chosen -- so the upper-bound half of this check is
// structurally trivial to Clippy. It stays as an explicit, symmetric
// safety net rather than relying on readers noticing the coincidence.
#[allow(clippy::absurd_extreme_comparisons)]
const _: () = assert!(UNSET < VALID_RANGE_MIN || UNSET > VALID_RANGE_MAX);

/// Encodes a fixed local offset in minutes. Returns `None` if `offset_minutes`
/// is outside UTC-12:00..=UTC+14:00 or is not a multiple of 15 minutes.
pub const fn encode(offset_minutes: i16) -> Option<u8> {
    if offset_minutes % MINUTES_PER_QUARTER != 0 {
        return None;
    }
    let quarters = offset_minutes / MINUTES_PER_QUARTER;
    if quarters < OFFSET_QUARTER_MIN || quarters > OFFSET_QUARTER_MAX {
        return None;
    }
    Some((quarters + ENCODE_BIAS) as u8)
}

/// Decodes a stored RAM_byte value to a fixed local offset in minutes.
/// Returns `None` for [`UNSET`], the legacy `0xAA` marker, or any other byte
/// outside the valid encoded range — all of these mean "no valid offset".
pub const fn decode(byte: u8) -> Option<i16> {
    if byte < VALID_RANGE_MIN || byte > VALID_RANGE_MAX {
        return None;
    }
    let quarters = byte as i16 - ENCODE_BIAS;
    Some(quarters * MINUTES_PER_QUARTER)
}
