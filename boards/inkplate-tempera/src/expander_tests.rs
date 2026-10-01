//! Faults run through the production expander, including its retry/reset path.
extern crate std;

use super::*;
use core::future::{pending, Future};
use core::task::{Context, Poll, Waker};
use std::collections::VecDeque;
use std::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Fault;

#[derive(Clone, Copy)]
enum WriteOutcome {
    Ok,
    FailBefore,
    FailAfter,
    PendingBefore,
    PendingAfter,
}

#[derive(Debug, PartialEq)]
enum Event {
    Read(u8),
    Write(u8, u8),
    Reset,
}

struct FakeI2c {
    regs: [u8; 0x50],
    writes: VecDeque<WriteOutcome>,
    failing_reads: u8,
    events: Vec<Event>,
}

impl FakeI2c {
    fn new() -> Self {
        let mut regs = [0; 0x50];
        // Distinct unrelated bits in both ports must survive each operation.
        for (idx, address) in PCAL_REG_ADDRS.into_iter().enumerate() {
            regs[address as usize] = 0x25 ^ idx as u8;
        }
        Self {
            regs,
            writes: VecDeque::new(),
            failing_reads: 0,
            events: Vec::new(),
        }
    }
}

impl I2cOps for FakeI2c {
    type Error = Fault;

    async fn read(&mut self, _: u8, _: &mut [u8]) -> core::result::Result<(), Fault> {
        panic!("expander uses addressed register reads")
    }

    async fn write(&mut self, addr: u8, bytes: &[u8]) -> core::result::Result<(), Fault> {
        assert_eq!(addr, IO_INT_ADDR);
        assert_eq!(bytes.len(), 2);
        self.events.push(Event::Write(bytes[0], bytes[1]));
        let outcome = self.writes.pop_front().unwrap_or(WriteOutcome::Ok);
        if matches!(
            outcome,
            WriteOutcome::Ok | WriteOutcome::FailAfter | WriteOutcome::PendingAfter
        ) {
            self.regs[bytes[0] as usize] = bytes[1];
        }
        match outcome {
            WriteOutcome::Ok => Ok(()),
            WriteOutcome::FailBefore | WriteOutcome::FailAfter => Err(Fault),
            WriteOutcome::PendingBefore | WriteOutcome::PendingAfter => pending().await,
        }
    }

    async fn write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> core::result::Result<(), Fault> {
        assert_eq!(addr, IO_INT_ADDR);
        assert_eq!(bytes.len(), 1);
        assert_eq!(buffer.len(), 1);
        self.events.push(Event::Read(bytes[0]));
        if self.failing_reads > 0 {
            self.failing_reads -= 1;
            return Err(Fault);
        }
        buffer[0] = self.regs[bytes[0] as usize];
        Ok(())
    }

    async fn probe(&mut self, _: u8) -> core::result::Result<bool, Fault> {
        panic!("expander does not probe")
    }

    async fn reset(&mut self) -> core::result::Result<(), Fault> {
        self.events.push(Event::Reset);
        Ok(())
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        // Advance the production retry/wake Timer deterministically.
        embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(1));
    }
}

fn started() -> PcalExpander<FakeI2c> {
    let mut expander = PcalExpander::new(FakeI2c::new());
    block_on(expander.begin()).unwrap();
    expander.i2c.events.clear();
    expander
}

fn assert_reconciled_before_write(events: &[Event]) {
    let expected: Vec<_> = CACHED_REGISTERS
        .iter()
        .map(|idx| Event::Read(PCAL_REG_ADDRS[*idx]))
        .collect();
    assert_eq!(&events[..expected.len()], expected.as_slice());
    assert!(matches!(events[expected.len()], Event::Write(..)));
}

#[test]
fn initialization_addresses_only_owned_registers_and_preserves_other_bits() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = PcalExpander::new(FakeI2c::new());
    let initial = expander.i2c.regs;
    // A caller cannot accidentally use the all-zero cache before begin().
    block_on(expander.digital_write(0, true)).unwrap();
    assert_reconciled_before_write(&expander.i2c.events);
    let mut expected = initial;
    expected[0x02] |= 1;
    assert_eq!(expander.i2c.regs, expected);
    assert!(expander.cache_valid);
}

#[test]
fn failed_write_reconciles_whether_or_not_the_device_applied_it() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    for outcome in [WriteOutcome::FailBefore, WriteOutcome::FailAfter] {
        let mut expander = started();
        expander.i2c.writes.extend([outcome, outcome]);
        assert!(block_on(expander.digital_write(3, true)).is_err());
        assert!(!expander.cache_valid);
        let mut expected = expander.i2c.regs;
        expected[0x02] |= 1;
        expander.i2c.events.clear();
        block_on(expander.digital_write(0, true)).unwrap();
        assert_reconciled_before_write(&expander.i2c.events);
        assert_eq!(expander.i2c.regs, expected);
        assert!(expander.cache_valid);
    }
}

#[test]
fn every_partial_pin_mode_failure_reconciles_before_the_next_owner_update() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    for (mode, count) in [
        (PinMode::Input, 1),
        (PinMode::Output, 2),
        (PinMode::InputPullUp, 3),
        (PinMode::InputPullDown, 3),
    ] {
        for failed_write in 0..count {
            for outcome in [WriteOutcome::FailBefore, WriteOutcome::FailAfter] {
                let mut expander = started();
                // Make every intended FG_GPOUT bit change observable, including
                // output-low and pull-down clears rather than only set bits.
                expander.i2c.regs[0x07] &= !0x80;
                expander.i2c.regs[0x47] &= !0x80;
                expander.i2c.regs[0x49] &= !0x80;
                if matches!(mode, PinMode::Output) {
                    expander.i2c.regs[0x07] |= 0x80;
                    expander.i2c.regs[0x03] |= 0x80;
                }
                if matches!(mode, PinMode::InputPullDown) {
                    expander.i2c.regs[0x49] |= 0x80;
                }
                block_on(expander.begin()).unwrap();
                expander.i2c.events.clear();
                expander
                    .i2c
                    .writes
                    .extend(core::iter::repeat_n(WriteOutcome::Ok, failed_write));
                expander.i2c.writes.extend([outcome, outcome]);
                assert!(block_on(expander.pin_mode(FG_GPOUT, mode)).is_err());
                assert!(!expander.cache_valid);
                // The next caller updates another pin in the same port. Only
                // completed hardware changes from the failed operation survive.
                let mut expected = expander.i2c.regs;
                expected[0x07] |= 1 << 6;
                expected[0x47] |= 1 << 6;
                expected[0x49] |= 1 << 6;
                expander.i2c.events.clear();
                block_on(expander.pin_mode(14, PinMode::InputPullUp)).unwrap();
                assert_reconciled_before_write(&expander.i2c.events);
                assert_eq!(expander.i2c.regs, expected);
                assert!(expander.cache_valid);
            }
        }
    }
}

#[test]
fn cancelled_partial_wake_configuration_is_reconciled_before_panel_update() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    for outcome in [WriteOutcome::PendingBefore, WriteOutcome::PendingAfter] {
        for pending_write in 0..3 {
            let mut expander = started();
            expander
                .i2c
                .writes
                .extend(core::iter::repeat_n(WriteOutcome::Ok, pending_write));
            expander.i2c.writes.push_back(outcome);
            {
                let mut wake = core::pin::pin!(expander.wake_fuel_gauge());
                let mut cx = Context::from_waker(Waker::noop());
                assert!(wake.as_mut().poll(&mut cx).is_pending());
                // A panel deadline drops the future while the bus outcome is
                // unknown. The invalidation must have happened before awaiting.
            }
            assert!(!expander.cache_valid);
            let mut expected = expander.i2c.regs;
            expected[0x02] &= !(1 << 3);
            expander.i2c.events.clear();
            block_on(expander.digital_write(3, false)).unwrap();
            assert_reconciled_before_write(&expander.i2c.events);
            assert_eq!(expander.i2c.regs, expected);
        }
    }
}

#[test]
fn failed_reconciliation_never_writes_and_retries_on_the_next_operation() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = started();
    expander
        .i2c
        .writes
        .extend([WriteOutcome::FailBefore, WriteOutcome::FailBefore]);
    assert!(block_on(expander.digital_write(3, true)).is_err());
    let mut expected = expander.i2c.regs;
    expander.i2c.failing_reads = 4;
    expander.i2c.events.clear();
    for _ in 0..2 {
        assert!(block_on(expander.wake_fuel_gauge()).is_err());
        assert!(!expander.cache_valid);
        assert_eq!(expander.i2c.regs, expected);
        assert!(!expander
            .i2c
            .events
            .iter()
            .any(|event| matches!(event, Event::Write(..))));
    }
    expander.i2c.events.clear();
    expected[0x02] |= 1;
    block_on(expander.digital_write(0, true)).unwrap();
    assert_reconciled_before_write(&expander.i2c.events);
    assert_eq!(expander.i2c.regs, expected);
}

#[test]
fn successful_retry_commits_cache_and_preserves_reset_recovery() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = started();
    expander.i2c.writes.push_back(WriteOutcome::FailAfter);
    block_on(expander.digital_write(3, true)).unwrap();
    assert_eq!(
        expander.i2c.events,
        [
            Event::Write(0x02, 0x2f),
            Event::Reset,
            Event::Write(0x02, 0x2f)
        ]
    );
    assert!(expander.cache_valid);
    expander.i2c.events.clear();
    block_on(expander.digital_write(0, false)).unwrap();
    assert_eq!(expander.i2c.events, [Event::Write(0x02, 0x2e)]);
}

#[test]
fn repeated_fuel_gauge_wakes_touch_only_the_fg_gpout_bits() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = started();
    let mut expected = expander.i2c.regs;
    expected[0x07] |= 0x80;
    expected[0x47] |= 0x80;
    expected[0x49] |= 0x80;
    for _ in 0..3 {
        block_on(expander.wake_fuel_gauge()).unwrap();
        assert_eq!(expander.i2c.regs, expected);
        assert!(expander.cache_valid);
    }
    assert_eq!(expander.i2c.events.len(), 9);
    assert!(expander
        .i2c
        .events
        .iter()
        .all(|event| matches!(event, Event::Write(0x07 | 0x47 | 0x49, _))));
}

#[test]
fn input_read_remains_live_and_invalid_pins_do_not_touch_the_bus() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = started();
    expander.i2c.regs[0x01] = 0x80;
    assert!(block_on(expander.digital_read(FG_GPOUT)).unwrap());
    expander.i2c.regs[0x01] = 0;
    assert!(!block_on(expander.digital_read(FG_GPOUT)).unwrap());
    assert_eq!(expander.i2c.events, [Event::Read(0x01), Event::Read(0x01)]);
    expander.i2c.events.clear();
    assert!(matches!(
        block_on(expander.digital_write(16, true)),
        Err(InkplateHalError::InvalidPin(16))
    ));
    assert!(matches!(
        block_on(expander.pin_mode(16, PinMode::Input)),
        Err(InkplateHalError::InvalidPin(16))
    ));
    assert!(matches!(
        block_on(expander.digital_read(16)),
        Err(InkplateHalError::InvalidPin(16))
    ));
    assert!(expander.i2c.events.is_empty());
}

#[test]
fn buzzer_latch_precedes_output_enable_and_preserves_other_pins() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let mut expander = started();
    expander.i2c.regs[0x07] |= 0x10;
    block_on(expander.begin()).unwrap();
    expander.i2c.events.clear();
    let out = expander.i2c.regs[0x03];
    let cfg = expander.i2c.regs[0x07];
    assert_ne!(cfg & 0x10, 0);
    block_on(expander.set_buzzer_power(false)).unwrap();
    assert_eq!(
        expander.i2c.events,
        [
            Event::Write(0x03, out | 0x10),
            Event::Write(0x07, cfg & !0x10)
        ]
    );
    expander.i2c.events.clear();
    block_on(expander.set_buzzer_power(true)).unwrap();
    assert_eq!(expander.i2c.events, [Event::Write(0x03, out & !0x10)]);
    expander.i2c.events.clear();
    block_on(expander.set_buzzer_power(false)).unwrap();
    assert_eq!(expander.i2c.events, [Event::Write(0x03, out | 0x10)]);
    assert_eq!(expander.i2c.regs[0x03], out | 0x10);
    assert_eq!(expander.i2c.regs[0x07], cfg & !0x10);
}
