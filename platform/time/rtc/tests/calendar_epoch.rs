//! Gregorian calendar <-> epoch conversion, leap years, boundaries, and
//! weekday computation.

use rtc::calendar::{self, Calendar};

#[test]
fn round_trips_an_ordinary_date() {
    let calendar = Calendar::new(2026, 8, 17, 12, 34, 56).expect("valid date");
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    assert_eq!(Calendar::from_epoch_seconds(epoch), Some(calendar));
}

#[test]
fn epoch_zero_is_1970_but_out_of_the_2000_2099_window() {
    // The RTC's two-digit BCD year cannot represent 1970, so epoch 0 must be
    // rejected even though it is a perfectly ordinary Unix timestamp.
    assert_eq!(Calendar::from_epoch_seconds(0), None);
}

#[test]
fn accepts_the_min_year_boundary() {
    let calendar = Calendar::new(2000, 1, 1, 0, 0, 0).expect("2000-01-01 is valid");
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    assert_eq!(Calendar::from_epoch_seconds(epoch), Some(calendar));
}

#[test]
fn accepts_the_max_year_boundary() {
    let calendar = Calendar::new(2099, 12, 31, 23, 59, 59).expect("2099-12-31 is valid");
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    assert_eq!(Calendar::from_epoch_seconds(epoch), Some(calendar));
}

#[test]
fn rejects_years_outside_the_window() {
    assert_eq!(Calendar::new(1999, 12, 31, 0, 0, 0), None);
    assert_eq!(Calendar::new(2100, 1, 1, 0, 0, 0), None);
}

#[test]
fn one_second_past_the_max_boundary_rolls_out_of_range() {
    let last_valid = Calendar::new(2099, 12, 31, 23, 59, 59)
        .expect("valid")
        .to_epoch_seconds()
        .expect("valid epoch");
    assert_eq!(Calendar::from_epoch_seconds(last_valid + 1), None);
}

#[test]
fn handles_ordinary_and_century_leap_years() {
    // 2024 and 2000 are leap years (2000 divisible by 400); 1900-style
    // century-not-divisible-by-400 years are outside our window, so the
    // in-window century check is exercised at 2000 itself.
    assert!(calendar::is_leap_year(2024));
    assert!(calendar::is_leap_year(2000));
    assert!(!calendar::is_leap_year(2023));
    assert!(!calendar::is_leap_year(2100));

    assert_eq!(calendar::days_in_month(2024, 2), Some(29));
    assert_eq!(calendar::days_in_month(2023, 2), Some(28));
}

#[test]
fn rejects_a_day_that_does_not_exist_in_the_month() {
    assert_eq!(Calendar::new(2023, 2, 29, 0, 0, 0), None); // not a leap year
    assert_eq!(Calendar::new(2024, 2, 30, 0, 0, 0), None);
    assert_eq!(Calendar::new(2026, 4, 31, 0, 0, 0), None);
    assert_eq!(Calendar::new(2026, 0, 1, 0, 0, 0), None);
    assert_eq!(Calendar::new(2026, 13, 1, 0, 0, 0), None);
    assert_eq!(Calendar::new(2026, 8, 0, 0, 0, 0), None);
}

#[test]
fn rejects_out_of_range_time_of_day() {
    assert_eq!(Calendar::new(2026, 8, 17, 24, 0, 0), None);
    assert_eq!(Calendar::new(2026, 8, 17, 0, 60, 0), None);
    assert_eq!(Calendar::new(2026, 8, 17, 0, 0, 60), None);
}

#[test]
fn computes_known_weekdays() {
    // 1970-01-01 was a Thursday.
    assert_eq!(calendar::weekday_of(1970, 1, 1), 4);
    // 2000-01-01 was a Saturday.
    assert_eq!(calendar::weekday_of(2000, 1, 1), 6);
    // 2026-08-17 (today, per this session) was a Monday.
    assert_eq!(calendar::weekday_of(2026, 8, 17), 1);
}

#[test]
fn bcd_round_trips_every_value_0_to_99() {
    for value in 0..=99u8 {
        let byte = calendar::bcd_encode(value).expect("0..=99 always encodes");
        assert_eq!(calendar::bcd_decode(byte), Some(value));
    }
}

#[test]
fn bcd_rejects_out_of_range_and_non_bcd_bytes() {
    assert_eq!(calendar::bcd_encode(100), None);
    // 0x0A..0x0F and 0xA0..0xFF nibbles are not valid decimal digits.
    assert_eq!(calendar::bcd_decode(0x0A), None);
    assert_eq!(calendar::bcd_decode(0xA0), None);
    assert_eq!(calendar::bcd_decode(0xFF), None);
}
