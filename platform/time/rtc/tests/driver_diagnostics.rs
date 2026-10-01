//! Failure attribution for `Pcf85063a`'s diagnostic sink: every failed
//! transaction reports its stage, register, and device address with the
//! original error, while wire labels, ordering, and recovery stay unchanged.

#[allow(dead_code)]
mod support;

use rtc::{
    calendar::Calendar,
    driver::{Pcf85063a, RtcDiagnosticSink, RtcError, RtcStage},
    offset,
    registers::{self, control_1},
};
use support::{block_on, FakeI2c};

const REQUEST_OFFSET_MINUTES: i16 = 60;

fn request_epoch() -> u32 {
    Calendar::new(2026, 8, 17, 12, 34, 56)
        .expect("valid test date")
        .to_epoch_seconds()
        .expect("valid epoch")
}

#[derive(Debug, Default)]
struct RecordingSink {
    events: std::vec::Vec<(RtcStage, u8, u8, std::string::String)>,
}

impl RtcDiagnosticSink for RecordingSink {
    fn on_i2c_error<E>(&mut self, stage: RtcStage, register: u8, address: u8, error: &E)
    where
        E: core::fmt::Debug,
    {
        self.events
            .push((stage, register, address, std::format!("{error:?}")));
    }
}

impl RtcDiagnosticSink for &mut RecordingSink {
    fn on_i2c_error<E>(&mut self, stage: RtcStage, register: u8, address: u8, error: &E)
    where
        E: core::fmt::Debug,
    {
        (**self).on_i2c_error(stage, register, address, error);
    }
}

#[test]
fn snapshot_read_failure_reports_exact_attribution_without_changing_the_label() {
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(0);
    let mut sink = RecordingSink::default();
    let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let result = block_on(driver.read_snapshot());

    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(result.unwrap_err().label(), "i2c");
    assert_eq!(
        sink.events,
        std::vec![(
            RtcStage::SnapshotRead,
            registers::BLOCK_START,
            registers::I2C_ADDRESS,
            std::format!("{:?}", support::FakeI2cError),
        )]
    );
}

#[test]
fn each_time_set_transaction_reports_its_own_stage() {
    let expected: [(RtcStage, u8); 7] = [
        (RtcStage::InvalidateMarker, registers::RAM_BYTE),
        (RtcStage::ReadControl, registers::CONTROL_1),
        (RtcStage::AssertStop, registers::CONTROL_1),
        (RtcStage::WriteCalendar, registers::CALENDAR_START),
        (RtcStage::ReleaseStop, registers::CONTROL_1),
        (RtcStage::WriteOffset, registers::RAM_BYTE),
        (RtcStage::ImmediateReadback, registers::BLOCK_START),
    ];
    for (fail_index, (stage, register)) in expected.iter().enumerate() {
        let mut fake = FakeI2c::new();
        fake.fail_at_transaction = Some(fail_index);
        let mut sink = RecordingSink::default();
        let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
        let result = block_on(driver.time_set(request_epoch(), REQUEST_OFFSET_MINUTES));

        assert!(
            matches!(result, Err(RtcError::I2c(_))),
            "index {fail_index}"
        );
        assert_eq!(result.unwrap_err().label(), "i2c");
        assert_eq!(
            sink.events,
            std::vec![(
                *stage,
                *register,
                registers::I2C_ADDRESS,
                std::format!("{:?}", support::FakeI2cError),
            )],
            "exactly one attribution at index {fail_index}"
        );
    }
}

#[test]
fn successful_operations_emit_no_diagnostics() {
    let mut fake = FakeI2c::new();
    let mut sink = RecordingSink::default();
    let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    block_on(driver.time_set(request_epoch(), REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");
    block_on(driver.read_snapshot()).expect("read succeeds");
    assert!(sink.events.is_empty());
}

#[test]
fn failed_immediate_readback_is_distinguishable_from_a_snapshot_read() {
    // Fail the post-write verification read: the driver must attribute it as
    // the immediate readback, not an ordinary snapshot read, and still leave
    // the offset marker unset.
    let mut fake = FakeI2c::new();
    fake.fail_at_transaction = Some(6);
    let mut sink = RecordingSink::default();
    let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let result = block_on(driver.time_set(request_epoch(), REQUEST_OFFSET_MINUTES));

    assert!(matches!(result, Err(RtcError::I2c(_))));
    assert_eq!(
        sink.events,
        std::vec![(
            RtcStage::ImmediateReadback,
            registers::BLOCK_START,
            registers::I2C_ADDRESS,
            std::format!("{:?}", support::FakeI2cError),
        )]
    );
    assert_eq!(fake.register(registers::RAM_BYTE), offset::UNSET);
}

#[test]
fn failed_cleanup_reports_failure_invalidate_without_changing_the_returned_error() {
    // Corrupt the readback so verification mismatches, then fail the
    // best-effort cleanup write itself: the caller still sees `Verify`, while
    // the sink records where the cleanup failed.
    let mut fake = FakeI2c::new();
    let calendar = Calendar::new(2026, 8, 17, 12, 34, 56).expect("valid test date");
    let mut block = std::vec![0u8; registers::BLOCK_LEN];
    block[registers::BLOCK_OFFSET_SECONDS] =
        rtc::calendar::bcd_encode(calendar.second.wrapping_add(1) % 60).expect("valid second");
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
    block[registers::BLOCK_OFFSET_RAM_BYTE] =
        offset::encode(REQUEST_OFFSET_MINUTES).expect("valid offset");
    fake.read_override = Some((1, block));
    // Transactions 0..=6 succeed; index 7 is the cleanup invalidation.
    fake.fail_at_transaction = Some(7);
    let mut sink = RecordingSink::default();
    let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let result = block_on(driver.time_set(request_epoch(), REQUEST_OFFSET_MINUTES));

    assert_eq!(result, Err(RtcError::Verify));
    assert_eq!(
        sink.events,
        std::vec![(
            RtcStage::FailureInvalidate,
            registers::RAM_BYTE,
            registers::I2C_ADDRESS,
            std::format!("{:?}", support::FakeI2cError),
        )]
    );
}

#[test]
fn failure_safe_ordering_is_intact_with_diagnostics_attached() {
    let mut fake = FakeI2c::new();
    let mut sink = RecordingSink::default();
    let mut driver = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    block_on(driver.time_set(request_epoch(), REQUEST_OFFSET_MINUTES)).expect("time_set succeeds");

    assert_eq!(
        fake.trace.first(),
        Some(&support::RecordedTransaction::Write {
            start: registers::RAM_BYTE,
            bytes: std::vec![offset::UNSET],
        }),
        "marker invalidation still leads"
    );
    assert_eq!(fake.register(registers::CONTROL_1) & control_1::STOP, 0);
    assert!(sink.events.is_empty());
}
