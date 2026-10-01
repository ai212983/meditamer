//! `apply_and_verify` through a real `Pcf85063a` carrying a diagnostic
//! sink: wire results stay stable while failures gain exact attribution,
//! and the delayed snapshot read is distinguishable from `time_set`'s own
//! immediate readback.

#[allow(dead_code)]
mod support;

use embedded_hal::i2c::{Error as I2cErrorTrait, ErrorKind, ErrorType, Operation};
use embedded_hal_async::i2c::I2c;
use rtc::{
    calendar::Calendar,
    driver::{Pcf85063a, RtcDiagnosticSink, RtcStage},
    registers,
};
use support::{block_on, FakeDelay};
use wall_clock::message::{
    SyncReply, SyncStatus, TriggerReason, VerifyErrorReason, PROTOCOL_VERSION,
};
use wall_clock::rtc_backend::apply_and_verify;
use wall_clock::session::{Coordinator, ReplyOutcome, SourceId, SourceSet};

const SOURCE: SourceId = SourceId(0);
const OFFSET_MINUTES: i16 = 60;

fn reply_utc() -> u32 {
    Calendar::new(2026, 8, 17, 12, 34, 56)
        .expect("valid test date")
        .to_epoch_seconds()
        .expect("valid epoch")
}

fn open_and_accept(coordinator: &mut Coordinator) -> SyncReply {
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        1,
        0,
        10_000,
        SourceSet::single(SOURCE),
    );
    let reply = SyncReply {
        version: PROTOCOL_VERSION,
        session: request.session,
        nonce: request.nonce,
        utc_epoch_seconds: reply_utc(),
        offset_minutes: OFFSET_MINUTES,
    };
    assert_eq!(
        coordinator.on_reply(SOURCE, &reply, 0),
        ReplyOutcome::Accepted
    );
    reply
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FakeError;

impl I2cErrorTrait for FakeError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

struct FakeI2c {
    registers: [u8; 256],
    fail_at: Option<usize>,
    transactions: usize,
}

impl FakeI2c {
    fn new() -> Self {
        Self {
            registers: [0u8; 256],
            fail_at: None,
            transactions: 0,
        }
    }
}

impl ErrorType for FakeI2c {
    type Error = FakeError;
}

impl I2c for FakeI2c {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), FakeError> {
        if address != registers::I2C_ADDRESS {
            return Err(FakeError);
        }
        let index = self.transactions;
        self.transactions += 1;
        if self.fail_at == Some(index) {
            return Err(FakeError);
        }
        let mut pointer: u8 = 0;
        for operation in operations.iter_mut() {
            match operation {
                Operation::Write(bytes) => {
                    let Some((&reg, data)) = bytes.split_first() else {
                        continue;
                    };
                    let mut addr = reg;
                    for &byte in data {
                        self.registers[addr as usize] = byte;
                        addr = addr.wrapping_add(1);
                    }
                    pointer = reg.wrapping_add(data.len() as u8);
                }
                Operation::Read(buf) => {
                    let mut addr = pointer;
                    for slot in buf.iter_mut() {
                        *slot = self.registers[addr as usize];
                        addr = addr.wrapping_add(1);
                    }
                    pointer = addr;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct RecordingSink {
    events: std::vec::Vec<(RtcStage, u8, u8)>,
}

impl RtcDiagnosticSink for RecordingSink {
    fn on_i2c_error<E>(&mut self, stage: RtcStage, register: u8, address: u8, _error: &E)
    where
        E: core::fmt::Debug,
    {
        self.events.push((stage, register, address));
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
fn time_set_i2c_failure_keeps_the_wire_reason_and_attributes_the_stage() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator);
    let mut fake = FakeI2c::new();
    fake.fail_at = Some(0);
    let mut sink = RecordingSink::default();
    let mut backend = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Rtc("i2c")));
    assert_eq!(
        sink.events,
        std::vec![(
            RtcStage::InvalidateMarker,
            registers::RAM_BYTE,
            registers::I2C_ADDRESS,
        )]
    );
}

#[test]
fn delayed_snapshot_failure_attributes_snapshot_read_not_the_immediate_readback() {
    // Transactions 0..=6 are `time_set` (including its own readback);
    // index 7 is the delayed verification read.
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator);
    let mut fake = FakeI2c::new();
    fake.fail_at = Some(7);
    let mut sink = RecordingSink::default();
    let mut backend = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Rtc("i2c")));
    assert_eq!(
        sink.events,
        std::vec![(
            RtcStage::SnapshotRead,
            registers::BLOCK_START,
            registers::I2C_ADDRESS,
        )]
    );
}

#[test]
fn successful_path_through_the_real_driver_emits_no_diagnostics() {
    // The fake clock never advances, so verification deterministically
    // reports `NotAdvanced` -- proving end-to-end delegation works with a
    // sink attached and stays silent when nothing fails.
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator);
    let mut fake = FakeI2c::new();
    let mut sink = RecordingSink::default();
    let mut backend = Pcf85063a::with_diagnostics(&mut fake, &mut sink);
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::NotAdvanced));
    assert!(sink.events.is_empty());
}
