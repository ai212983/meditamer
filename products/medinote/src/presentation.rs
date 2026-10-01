//! Hardware-independent content formatting for Medinote screens.

/// 0% at 3.0V, 100% at 4.12V, linear between. Waveshare's own firmware uses
/// exactly this curve (`Adc_GetBatteryLevel` in their ADC example) for the
/// same 18650 cell this board holds, so it is reused rather than re-derived --
/// a fuel gauge this is not, but it is the vendor's own answer for this pack.
pub fn battery_percent_from_mv(battery_mv: u32) -> u8 {
    const EMPTY_MV: u32 = 3_000;
    const FULL_MV: u32 = 4_120;
    if battery_mv <= EMPTY_MV {
        0
    } else if battery_mv >= FULL_MV {
        100
    } else {
        (((battery_mv - EMPTY_MV) * 100) / (FULL_MV - EMPTY_MV)) as u8
    }
}

/// Render `84%  4.05V` into `buffer`, NUL-terminated, returning the length
/// including the NUL.
pub fn format_battery(buffer: &mut [u8; 24], percent: u8, battery_mv: u32) -> usize {
    let mut writer = FixedWriter::<'_, 24>::new(buffer);
    writer.u8(percent);
    writer.str("%  ");
    writer.volts(battery_mv);
    writer.str("V");
    writer.push(0);
    writer.at
}

/// `HH:MM` from an epoch timestamp, NUL-terminated; returns the length
/// including the NUL. Minutes only: this screen redraws once a minute, so a
/// seconds digit would just sit there implying a precision the update rate
/// does not deliver.
pub fn format_time_hm(buffer: &mut [u8; 8], epoch_seconds: u32) -> usize {
    let seconds_today = epoch_seconds % 86_400;
    let hours = seconds_today / 3_600;
    let minutes = (seconds_today % 3_600) / 60;
    buffer[0] = b'0' + (hours / 10) as u8;
    buffer[1] = b'0' + (hours % 10) as u8;
    buffer[2] = b':';
    buffer[3] = b'0' + (minutes / 10) as u8;
    buffer[4] = b'0' + (minutes % 10) as u8;
    buffer[5] = 0;
    6
}

/// A `u32` as decimal digits, NUL-terminated; returns the length including
/// the NUL. `buffer` must hold at least 11 bytes (10 digits plus the NUL --
/// `u32::MAX` is 10 digits). Hand-rolled for the same reason
/// [`format_reading`] is: no `core::fmt` machinery for one fixed-width
/// integer.
pub fn format_count(buffer: &mut [u8; 11], count: u32) -> usize {
    if count == 0 {
        buffer[0] = b'0';
        buffer[1] = 0;
        return 2;
    }
    let mut digits = [0u8; 10];
    let mut value = count;
    let mut len = 0;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for (index, digit) in digits[..len].iter().rev().enumerate() {
        buffer[index] = *digit;
    }
    buffer[len] = 0;
    len + 1
}

/// Render `21.4 C   47.8 %` into `buffer`, NUL-terminated, returning the length
/// including the NUL. Hand-rolled because `core::fmt` into a fixed buffer costs
/// more code than these two fixed-point conversions do.
pub fn format_reading(buffer: &mut [u8; 32], temperature_mc: i32, humidity_mpct: i32) -> usize {
    let mut writer = FixedWriter::<'_, 32>::new(buffer);
    writer.tenths(temperature_mc);
    writer.str(" C   ");
    writer.tenths(humidity_mpct);
    writer.str(" %");
    writer.push(0);
    writer.at
}

struct FixedWriter<'a, const N: usize> {
    buffer: &'a mut [u8; N],
    at: usize,
}

impl<'a, const N: usize> FixedWriter<'a, N> {
    const fn new(buffer: &'a mut [u8; N]) -> Self {
        Self { buffer, at: 0 }
    }

    fn push(&mut self, byte: u8) {
        if self.at < N - 1 {
            self.buffer[self.at] = byte;
            self.at += 1;
        }
    }

    fn str(&mut self, text: &str) {
        for byte in text.as_bytes() {
            self.push(*byte);
        }
    }

    fn u8(&mut self, value: u8) {
        if value >= 100 {
            self.push(b'0' + (value / 100) % 10);
        }
        if value >= 10 {
            self.push(b'0' + (value / 10) % 10);
        }
        self.push(b'0' + value % 10);
    }

    /// One decimal place, from thousandths.
    fn tenths(&mut self, value: i32) {
        let magnitude = value.unsigned_abs();
        let whole = magnitude / 1000;
        let tenth = (magnitude % 1000) / 100;
        if value < 0 {
            self.push(b'-');
        }
        if whole >= 100 {
            self.push(b'0' + (whole / 100 % 10) as u8);
        }
        if whole >= 10 {
            self.push(b'0' + (whole / 10 % 10) as u8);
        }
        self.push(b'0' + (whole % 10) as u8);
        self.push(b'.');
        self.push(b'0' + tenth as u8);
    }

    /// One decimal place, from millivolts, as volts.
    fn volts(&mut self, millivolts: u32) {
        self.push(b'0' + (((millivolts / 1000) % 10) as u8));
        self.push(b'.');
        self.push(b'0' + ((millivolts / 100) % 10) as u8);
        self.push(b'0' + ((millivolts / 10) % 10) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_str(buffer: &[u8], len: usize) -> &str {
        core::str::from_utf8(&buffer[..len - 1]).unwrap()
    }

    #[test]
    fn battery_percent_clamps_below_empty() {
        assert_eq!(battery_percent_from_mv(2_500), 0);
        assert_eq!(battery_percent_from_mv(3_000), 0);
    }

    #[test]
    fn battery_percent_clamps_above_full() {
        assert_eq!(battery_percent_from_mv(4_120), 100);
        assert_eq!(battery_percent_from_mv(4_500), 100);
    }

    #[test]
    fn battery_percent_interpolates_linearly() {
        // Midpoint of the 3000-4120mV range is 3560mV -> 50%.
        assert_eq!(battery_percent_from_mv(3_560), 50);
    }

    #[test]
    fn format_battery_renders_percent_and_volts() {
        let mut buf = [0u8; 24];
        let len = format_battery(&mut buf, 84, 4_050);
        assert_eq!(as_str(&buf, len), "84%  4.05V");
    }

    #[test]
    fn format_battery_pads_single_digit_percent() {
        let mut buf = [0u8; 24];
        let len = format_battery(&mut buf, 5, 3_200);
        assert_eq!(as_str(&buf, len), "5%  3.20V");
    }

    #[test]
    fn format_time_hm_renders_midnight() {
        let mut buf = [0u8; 8];
        let len = format_time_hm(&mut buf, 0);
        assert_eq!(as_str(&buf, len), "00:00");
    }

    #[test]
    fn format_time_hm_wraps_past_a_day() {
        let mut buf = [0u8; 8];
        // 25 hours in -> same as 1 hour in.
        let len = format_time_hm(&mut buf, 25 * 3_600);
        assert_eq!(as_str(&buf, len), "01:00");
    }

    #[test]
    fn format_count_renders_zero() {
        let mut buf = [0u8; 11];
        let len = format_count(&mut buf, 0);
        assert_eq!(as_str(&buf, len), "0");
    }

    #[test]
    fn format_count_renders_multiple_digits_without_leading_zeros() {
        let mut buf = [0u8; 11];
        let len = format_count(&mut buf, 4_002);
        assert_eq!(as_str(&buf, len), "4002");
    }

    #[test]
    fn format_count_renders_the_maximum_u32() {
        let mut buf = [0u8; 11];
        let len = format_count(&mut buf, u32::MAX);
        assert_eq!(as_str(&buf, len), "4294967295");
    }

    #[test]
    fn format_reading_renders_temperature_and_humidity() {
        let mut buf = [0u8; 32];
        let len = format_reading(&mut buf, 21_400, 47_800);
        assert_eq!(as_str(&buf, len), "21.4 C   47.8 %");
    }

    #[test]
    fn format_reading_renders_negative_temperature() {
        let mut buf = [0u8; 32];
        let len = format_reading(&mut buf, -5_300, 30_000);
        assert_eq!(as_str(&buf, len), "-5.3 C   30.0 %");
    }
}
