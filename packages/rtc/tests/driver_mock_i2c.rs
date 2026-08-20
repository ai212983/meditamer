//! Mock-I2C coverage for `Pcf85063a`: single-burst calendar access, marker
//! invalidation ordering, STOP assert/release ordering, partial failures at
//! every transaction step, readback verification, and `TIMEGET` reason
//! precedence.

mod support;

use rtc::{
    calendar::Calendar,
    driver::{Pcf85063a, RtcError, TimeSetOutcome, UnavailableReason},
    offset,
    registers::{self, control_1, seconds},
};
use support::{block_on, FakeI2c, RecordedTransaction};

fn request_calendar() -> Calendar {
    Calendar::new(2026, 8, 17, 12, 34, 56).expect("valid test date")
}

const REQUEST_OFFSET_MINUTES: i16 = 60;

/// Builds a self-consistent 11-byte `Control_1..=Years` block matching
/// `calendar`/`offset_minutes`, with `Control_1`/`STOP` and the
/// oscillator-stop flag clear -- the "everything agrees" baseline that
/// mismatch tests then perturb.
fn valid_block(calendar: &Calendar, offset_minutes: i16) -> std::vec::Vec<u8> {
    let mut block = std::vec![0u8; registers::BLOCK_LEN];
    block[registers::BLOCK_OFFSET_RAM_BYTE] = offset::encode(offset_minutes).expect("valid offset");
    block[registers::BLOCK_OFFSET_SECONDS] =
        rtc::calendar::bcd_encode(calendar.second).expect("valid second");
    block[registers::BLOCK_OFFSET_MINUTES] =
        rtc::calendar::bcd_encode(calendar.minute).expect("valid minute");
    block[registers::BLOCK_OFFSET_HOURS] =
        rtc::calendar::bcd_encode(calendar.hour).expect("valid hour");
    block[registers::BLOCK_OFFSET_DAYS] =
        rtc::calendar::bcd_encode(calendar.day).expect("valid day");
    block[registers::BLOCK_OFFSET_WEEKDAYS] = calendar.weekday();
    block[registers::BLOCK_OFFSET_MONTHS] =
        rtc::calendar::bcd_encode(calendar.month).expect("valid month");
    block[registers::BLOCK_OFFSET_YEARS] =
        rtc::calendar::bcd_encode((calendar.year - rtc::calendar::MIN_YEAR) as u8)
            .expect("valid year");
    block
}

// --- Successful TIMESET: ordering and single-burst access ---------------

#[test]
fn time_set_invalidates_the_offset_marker_before_anything_else() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    assert_eq!(
        fake.trace.first(),
        Some(&RecordedTransaction::Write {
            start: registers::RAM_BYTE,
            bytes: std::vec![offset::UNSET],
        }),
        "the very first bus transaction must invalidate the offset marker"
    );
}

#[test]
fn time_set_asserts_stop_before_the_calendar_write_and_releases_after() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    let assert_stop_index = fake
        .trace
        .iter()
        .position(|entry| matches!(entry, RecordedTransaction::Write { start, bytes }
            if *start == registers::CONTROL_1 && bytes.first().is_some_and(|b| b & control_1::STOP != 0)))
        .expect("an assert-STOP write occurred");
    let calendar_write_index = fake
        .trace
        .iter()
        .position(|entry| {
            matches!(entry, RecordedTransaction::Write { start, bytes }
                if *start == registers::CALENDAR_START && bytes.len() == registers::CALENDAR_LEN)
        })
        .expect("a single-burst calendar write occurred");
    let release_stop_index = fake
        .trace
        .iter()
        .enumerate()
        .position(|(index, entry)| {
            index > calendar_write_index
                && matches!(entry, RecordedTransaction::Write { start, bytes }
                    if *start == registers::CONTROL_1 && bytes.first().is_some_and(|b| b & control_1::STOP == 0))
        })
        .expect("a release-STOP write occurred after the calendar write");

    assert!(assert_stop_index < calendar_write_index);
    assert!(calendar_write_index < release_stop_index);
}

#[test]
fn time_set_writes_the_calendar_in_a_single_transaction() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    let calendar_writes = fake
        .trace
        .iter()
        .filter(|entry| matches!(entry, RecordedTransaction::Write { start, .. } if *start == registers::CALENDAR_START))
        .count();
    assert_eq!(calendar_writes, 1);
}

#[test]
fn time_set_reads_back_the_full_block_in_a_single_burst() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    let final_read = fake
        .trace
        .iter()
        .rfind(|entry| matches!(entry, RecordedTransaction::Read { .. }));
    assert_eq!(
        final_read,
        Some(&RecordedTransaction::Read {
            start: registers::BLOCK_START,
            len: registers::BLOCK_LEN,
        })
    );
}

#[test]
fn time_set_normalizes_control_1_preserving_only_cie_and_cap_sel() {
    let mut fake = FakeI2c::new();
    // EXT_TEST, unused bit 6, SR, unused bit 3, and 12/24 all set; CIE and
    // CAP_SEL also set. All but CIE/CAP_SEL must be forced to zero.
    fake.set_register(registers::CONTROL_1, 0xDF);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    let outcome =
        block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    assert_eq!(
        outcome,
        TimeSetOutcome {
            utc_epoch_seconds: epoch,
            offset_minutes: REQUEST_OFFSET_MINUTES,
        }
    );
    let final_control_1 = fake.register(registers::CONTROL_1);
    assert_eq!(final_control_1, control_1::CIE | control_1::CAP_SEL);
    assert_eq!(
        final_control_1 & control_1::STOP,
        0,
        "STOP must be released"
    );
    assert_eq!(
        final_control_1 & control_1::HOUR_MODE_12,
        0,
        "24-hour mode must be forced"
    );
}

#[test]
fn time_set_writes_the_encoded_offset_and_readback_reason_is_none() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    assert_eq!(
        offset::decode(fake.register(registers::RAM_BYTE)),
        Some(REQUEST_OFFSET_MINUTES)
    );
}

// --- Input validation never touches the bus ------------------------------

#[test]
fn time_set_rejects_an_out_of_range_epoch_without_any_bus_traffic() {
    let mut fake = FakeI2c::new();
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(0, 0)); // 1970: outside 2000-2099
    assert_eq!(result, Err(RtcError::Range));
    assert!(fake.trace.is_empty());
}

#[test]
fn time_set_rejects_an_offset_with_bad_granularity_without_any_bus_traffic() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, 7));
    assert_eq!(result, Err(RtcError::Offset));
    assert!(fake.trace.is_empty());
}

// --- Partial I2C failures at each transaction step ------------------------

#[test]
fn time_set_fails_at_marker_invalidation_leaves_the_bus_untouched_by_control_1() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(0);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(fake.register(registers::CONTROL_1), 0, "never reached");
}

#[test]
fn time_set_fails_reading_control_1_before_asserting_stop() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(1);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(fake.register(registers::CONTROL_1) & control_1::STOP, 0);
}

#[test]
fn time_set_fails_asserting_stop_and_recovers_control_1() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(2);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    // The recovery write (best-effort release, per the plan) must still
    // have landed even though the assert-STOP write itself failed.
    assert_eq!(
        fake.trace.last(),
        Some(&RecordedTransaction::Write {
            start: registers::CONTROL_1,
            bytes: std::vec![0],
        })
    );
    assert_eq!(fake.register(registers::CONTROL_1) & control_1::STOP, 0);
    assert_eq!(fake.register(registers::RAM_BYTE), offset::UNSET);
}

#[test]
fn time_set_fails_the_calendar_write_and_recovers_control_1() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(3);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(
        fake.register(registers::CONTROL_1) & control_1::STOP,
        0,
        "STOP must be released even though the calendar write failed"
    );
    assert_eq!(fake.register(registers::RAM_BYTE), offset::UNSET);
}

#[test]
fn time_set_fails_releasing_stop_and_leaves_the_clock_stopped() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(4);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    // The release write itself is what failed: nothing further to retry.
    assert_ne!(fake.register(registers::CONTROL_1) & control_1::STOP, 0);
    assert_eq!(fake.register(registers::RAM_BYTE), offset::UNSET);
}

#[test]
fn time_set_fails_writing_the_offset_and_leaves_it_unset() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(5);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(fake.register(registers::CONTROL_1) & control_1::STOP, 0);
    assert_eq!(fake.register(registers::RAM_BYTE), offset::UNSET);
}

#[test]
fn time_set_fails_the_verify_read_after_writing_correct_values() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(6);
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert!(matches!(result, Err(RtcError::I2c(_))));
    // The device itself ended up in the requested state; only the
    // verification read failed.
    assert_eq!(
        offset::decode(fake.register(registers::RAM_BYTE)),
        Some(REQUEST_OFFSET_MINUTES)
    );
}

// --- Readback verification (data present, but wrong) ---------------------

#[test]
fn time_set_reports_verify_when_the_readback_calendar_disagrees() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut mismatched = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    // Flip the seconds field by one: still perfectly valid BCD, still a
    // decodable, self-consistent calendar -- just not the one requested.
    mismatched[registers::BLOCK_OFFSET_SECONDS] =
        rtc::calendar::bcd_encode(calendar.second.wrapping_add(1) % 60).expect("valid second");
    fake.read_override = Some((1, mismatched));

    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert_eq!(result, Err(RtcError::Verify));
}

#[test]
fn time_set_reports_verify_when_the_readback_offset_disagrees() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut mismatched = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    mismatched[registers::BLOCK_OFFSET_RAM_BYTE] =
        offset::encode(REQUEST_OFFSET_MINUTES + 15).expect("valid offset");
    fake.read_override = Some((1, mismatched));

    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert_eq!(result, Err(RtcError::Verify));
}

#[test]
fn time_set_reports_clock_stopped_when_readback_stop_bit_is_set() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_CONTROL_1] |= control_1::STOP;
    fake.read_override = Some((1, block));

    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert_eq!(result, Err(RtcError::ClockStopped));
}

#[test]
fn time_set_reports_clock_stopped_when_readback_oscillator_stop_flag_is_set() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let epoch = calendar.to_epoch_seconds().expect("valid epoch");
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_SECONDS] |= seconds::OS;
    fake.read_override = Some((1, block));

    let mut driver = Pcf85063a::new(&mut fake);
    let result = block_on(driver.time_set(epoch, REQUEST_OFFSET_MINUTES));
    assert_eq!(result, Err(RtcError::ClockStopped));
}

// --- TIMEGET / read_snapshot reason precedence ----------------------------

#[test]
fn read_snapshot_reports_valid_and_computes_local_time() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    fake.set_block(
        registers::BLOCK_START,
        &valid_block(&calendar, REQUEST_OFFSET_MINUTES),
    );

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(snapshot.valid);
    assert_eq!(snapshot.reason, None);
    let expected_utc = calendar.to_epoch_seconds().expect("valid epoch");
    assert_eq!(snapshot.utc_epoch_seconds, expected_utc);
    assert_eq!(snapshot.offset_minutes, REQUEST_OFFSET_MINUTES);
    assert_eq!(
        snapshot.local_epoch_seconds,
        ((expected_utc as i64) + (REQUEST_OFFSET_MINUTES as i64 * 60)) as u32
    );
}

#[test]
fn read_snapshot_performs_a_single_burst_read() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    fake.set_block(
        registers::BLOCK_START,
        &valid_block(&calendar, REQUEST_OFFSET_MINUTES),
    );

    let mut driver = Pcf85063a::new(&mut fake);
    block_on(driver.read_snapshot()).expect("read succeeds");

    assert_eq!(
        fake.trace,
        std::vec![
            RecordedTransaction::Write {
                start: registers::BLOCK_START,
                bytes: std::vec![],
            },
            RecordedTransaction::Read {
                start: registers::BLOCK_START,
                len: registers::BLOCK_LEN,
            },
        ]
    );
}

#[test]
fn read_snapshot_treats_stop_bit_as_higher_precedence_than_oscillator_stop() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_CONTROL_1] |= control_1::STOP;
    block[registers::BLOCK_OFFSET_SECONDS] |= seconds::OS;
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::ClockStopped));
}

#[test]
fn read_snapshot_reports_oscillator_stopped_when_stop_bit_is_clear() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_SECONDS] |= seconds::OS;
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::OscillatorStopped));
}

#[test]
fn read_snapshot_reports_invalid_calendar_for_non_bcd_seconds() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_SECONDS] = 0x0A; // low nibble 0xA is not a decimal digit
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::InvalidCalendar));
}

#[test]
fn read_snapshot_reports_invalid_calendar_for_inconsistent_weekday() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_WEEKDAYS] = (calendar.weekday() + 1) % 7;
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::InvalidCalendar));
}

#[test]
fn read_snapshot_reports_offset_unset_when_marker_is_unset() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_RAM_BYTE] = offset::UNSET;
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::OffsetUnset));
}

#[test]
fn read_snapshot_reports_offset_unset_for_the_legacy_arduino_marker() {
    let mut fake = FakeI2c::new();
    let calendar = request_calendar();
    let mut block = valid_block(&calendar, REQUEST_OFFSET_MINUTES);
    block[registers::BLOCK_OFFSET_RAM_BYTE] = offset::LEGACY_ARDUINO_MARKER;
    fake.set_block(registers::BLOCK_START, &block);

    let mut driver = Pcf85063a::new(&mut fake);
    let snapshot = block_on(driver.read_snapshot()).expect("read succeeds");

    assert!(!snapshot.valid);
    assert_eq!(snapshot.reason, Some(UnavailableReason::OffsetUnset));
}

// --- Error labels match the wire protocol's stable reason vocabulary -----

#[test]
fn error_labels_match_the_wire_protocol_vocabulary() {
    assert_eq!(RtcError::<support::FakeI2cError>::Range.label(), "range");
    assert_eq!(RtcError::<support::FakeI2cError>::Offset.label(), "offset");
    assert_eq!(RtcError::I2c(support::FakeI2cError).label(), "i2c");
    assert_eq!(RtcError::<support::FakeI2cError>::Verify.label(), "verify");
    assert_eq!(
        RtcError::<support::FakeI2cError>::ClockStopped.label(),
        "clock_stopped"
    );
}

#[test]
fn unavailable_reason_labels_match_the_wire_protocol_vocabulary() {
    assert_eq!(
        UnavailableReason::OscillatorStopped.label(),
        "oscillator_stopped"
    );
    assert_eq!(UnavailableReason::ClockStopped.label(), "clock_stopped");
    assert_eq!(UnavailableReason::OffsetUnset.label(), "offset_unset");
    assert_eq!(
        UnavailableReason::InvalidCalendar.label(),
        "invalid_calendar"
    );
}
