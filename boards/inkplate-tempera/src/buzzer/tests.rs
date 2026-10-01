//! Host tests for the repaired pitch mapping, retried transport, and note
//! sequencing. The transport and sequencing under test here are the real
//! generic functions the firmware calls; only the millisecond waits and the
//! I2C/rail hardware are faked.
extern crate std;

use super::*;
use core::future::Future;
use core::task::{Context, Poll, Waker};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fault;

#[derive(Debug, Eq, PartialEq)]
enum BusEvent {
    Write { addr: u8, byte: u8 },
    Reset,
}

struct FakeI2c {
    addr: u8,
    failures_before_success: u32,
    events: Vec<BusEvent>,
}

impl FakeI2c {
    fn new(failures_before_success: u32) -> Self {
        Self {
            addr: 0x2F,
            failures_before_success,
            events: Vec::new(),
        }
    }
}

impl I2cOps for FakeI2c {
    type Error = Fault;

    async fn read(&mut self, _: u8, _: &mut [u8]) -> Result<(), Fault> {
        panic!("pitch programming uses writes only")
    }

    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Fault> {
        assert_eq!(addr, self.addr, "unexpected pitch-programming address");
        self.events.push(BusEvent::Write {
            addr,
            byte: bytes[0],
        });
        if self.failures_before_success > 0 {
            self.failures_before_success -= 1;
            return Err(Fault);
        }
        Ok(())
    }

    async fn write_read(&mut self, _: u8, _: &[u8], _: &mut [u8]) -> Result<(), Fault> {
        panic!("pitch programming uses writes only")
    }

    async fn probe(&mut self, _: u8) -> Result<bool, Fault> {
        panic!("pitch programming never probes")
    }

    async fn reset(&mut self) -> Result<(), Fault> {
        self.events.push(BusEvent::Reset);
        Ok(())
    }
}

struct FakeDelay {
    waits_ms: Vec<u64>,
}

impl FakeDelay {
    fn new() -> Self {
        Self {
            waits_ms: Vec::new(),
        }
    }
}

impl PitchWriteDelay for FakeDelay {
    async fn wait_ms(&mut self, ms: u64) {
        self.waits_ms.push(ms);
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

#[test]
fn nominal_endpoints_match_plan_values() {
    // Plan endpoints: approximately 586.4 and 3136.1 Hz from R59 = 2200
    // Ohm, assumed Rw = 100 Ohm, RAB = 10 kOhm, C62 = 100 nF.
    assert_eq!(NOMINAL_FREQ_MAX_HZ, 3136);
    assert_eq!(NOMINAL_FREQ_MIN_HZ, 586);
    assert_eq!(nominal_freq_hz(0), Some(3136));
    assert_eq!(nominal_freq_hz(127), Some(586));
    assert_eq!(nominal_freq_hz(128), None);
}

#[test]
fn mapping_is_monotonic_and_nearest_in_cents() {
    for code in 0..127u8 {
        assert!(nominal_freq_hz(code) >= nominal_freq_hz(code + 1));
    }
    // Plan check: the nearest code to 1000 Hz is 62.
    assert_eq!(code_for_frequency(1000), Ok(62));
    assert_eq!(code_for_frequency(NOMINAL_FREQ_MAX_HZ), Ok(0));
    assert_eq!(code_for_frequency(NOMINAL_FREQ_MIN_HZ), Ok(127));
}

#[test]
fn range_policy_saturates_and_rejects_before_division() {
    assert_eq!(code_for_frequency(5000), Ok(0));
    assert_eq!(code_for_frequency(i32::MAX), Ok(0));
    assert_eq!(code_for_frequency(100), Ok(127));
    assert_eq!(code_for_frequency(1), Ok(127));
    assert_eq!(code_for_frequency(0), Err(BuzzerFreqError::NonPositive(0)));
    assert_eq!(
        code_for_frequency(-440),
        Err(BuzzerFreqError::NonPositive(-440))
    );
    assert_eq!(
        code_for_frequency(i32::MIN),
        Err(BuzzerFreqError::NonPositive(i32::MIN))
    );
}

#[test]
fn successful_write_orders_single_payload_without_recovery() {
    let mut i2c = FakeI2c::new(0);
    let mut delay = FakeDelay::new();
    assert!(block_on(write_pitch_code(&mut i2c, 0x2F, 62, &mut delay)).is_ok());
    assert_eq!(
        i2c.events,
        std::vec![BusEvent::Write {
            addr: 0x2F,
            byte: 62
        }]
    );
    assert!(delay.waits_ms.is_empty());
}

#[test]
fn recovered_writes_retry_then_succeed() {
    let mut i2c = FakeI2c::new(2);
    let mut delay = FakeDelay::new();
    assert!(block_on(write_pitch_code(&mut i2c, 0x2F, 62, &mut delay)).is_ok());
    assert_eq!(
        i2c.events,
        std::vec![
            BusEvent::Write {
                addr: 0x2F,
                byte: 62
            },
            BusEvent::Reset,
            BusEvent::Write {
                addr: 0x2F,
                byte: 62
            },
            BusEvent::Reset,
            BusEvent::Write {
                addr: 0x2F,
                byte: 62
            },
        ]
    );
    assert_eq!(
        delay.waits_ms,
        std::vec![PITCH_RETRY_WAIT_MS, PITCH_RETRY_WAIT_MS]
    );
}

#[test]
fn exhausted_writes_propagate_without_unrelated_side_effects() {
    let mut i2c = FakeI2c::new(u32::MAX);
    let mut delay = FakeDelay::new();
    assert_eq!(
        block_on(write_pitch_code(&mut i2c, 0x2F, 62, &mut delay)),
        Err(Fault)
    );
    let writes = i2c
        .events
        .iter()
        .filter(|event| matches!(event, BusEvent::Write { .. }))
        .count();
    assert_eq!(writes, PITCH_WRITE_ATTEMPTS as usize);
    // Every write targets the rheostat with the selected code; the
    // frontlight digipot (0x2E) and every other address stay untouched, and
    // the old frontlight-enable recovery is gone.
    assert!(i2c.events.iter().all(|event| !matches!(
        event,
        BusEvent::Write { addr, .. } if *addr != 0x2F
    )));
    assert_eq!(delay.waits_ms.len(), (PITCH_WRITE_ATTEMPTS - 1) as usize);
}

#[derive(Debug, Eq, PartialEq)]
enum RailEvent {
    Rail(bool),
    Wait(u64),
    Write(u8),
}

struct FakeRail {
    events: Vec<RailEvent>,
    write_failures: u32,
    rail_calls: u32,
    fail_rail_on_call: Option<u32>,
    fail_all_rail: bool,
}

impl FakeRail {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            write_failures: 0,
            rail_calls: 0,
            fail_rail_on_call: None,
            fail_all_rail: false,
        }
    }

    fn failing_all_rail() -> Self {
        Self {
            events: Vec::new(),
            write_failures: 0,
            rail_calls: 0,
            fail_rail_on_call: None,
            fail_all_rail: true,
        }
    }
}

impl BuzzerRail for FakeRail {
    type Error = Fault;

    async fn set_rail_enabled(&mut self, enabled: bool) -> Result<(), Fault> {
        self.events.push(RailEvent::Rail(enabled));
        let call = self.rail_calls;
        self.rail_calls += 1;
        if self.fail_all_rail || self.fail_rail_on_call == Some(call) {
            return Err(Fault);
        }
        Ok(())
    }

    async fn write_code(&mut self, code: u8) -> Result<(), Fault> {
        self.events.push(RailEvent::Write(code));
        if self.write_failures > 0 {
            self.write_failures -= 1;
            return Err(Fault);
        }
        Ok(())
    }

    async fn wait_ms(&mut self, ms: u64) {
        self.events.push(RailEvent::Wait(ms));
    }
}

#[test]
fn start_note_powers_before_programming() {
    let mut rail = FakeRail::new();
    assert_eq!(block_on(start_note(&mut rail, 1000)), Ok(62));
    assert_eq!(
        rail.events,
        std::vec![
            RailEvent::Rail(true),
            RailEvent::Wait(STARTUP_WAIT_MS),
            RailEvent::Write(62),
        ]
    );
}

#[test]
fn invalid_request_touches_no_hardware() {
    let mut rail = FakeRail::new();
    assert_eq!(
        block_on(start_note(&mut rail, 0)),
        Err(BuzzerError::InvalidFrequency(0))
    );
    assert!(rail.events.is_empty());
    assert_eq!(
        block_on(retune(&mut rail, -440)),
        Err(BuzzerError::InvalidFrequency(-440))
    );
    assert!(rail.events.is_empty());
}

#[test]
fn rail_enable_failure_shuts_down_and_reports_original() {
    let mut rail = FakeRail::new();
    rail.fail_rail_on_call = Some(0);
    assert_eq!(
        block_on(start_note(&mut rail, 1000)),
        Err(BuzzerError::RailEnable(Fault))
    );
    // Ambiguous enable failure still attempts shutdown; no pitch write.
    assert_eq!(
        rail.events,
        std::vec![RailEvent::Rail(true), RailEvent::Rail(false)]
    );
}

#[test]
fn rail_enable_failure_with_failed_shutdown_reports_both() {
    let mut rail = FakeRail::failing_all_rail();
    assert_eq!(
        block_on(start_note(&mut rail, 1000)),
        Err(BuzzerError::RailEnableThenShutdown {
            enable: Fault,
            shutdown: Fault,
        })
    );
    assert_eq!(
        rail.events,
        std::vec![RailEvent::Rail(true), RailEvent::Rail(false)]
    );
}

#[test]
fn pitch_failure_shuts_down_and_reports_original() {
    let mut rail = FakeRail::new();
    rail.write_failures = 1;
    assert_eq!(
        block_on(start_note(&mut rail, 1000)),
        Err(BuzzerError::PitchWrite(Fault))
    );
    assert_eq!(
        rail.events,
        std::vec![
            RailEvent::Rail(true),
            RailEvent::Wait(STARTUP_WAIT_MS),
            RailEvent::Write(62),
            RailEvent::Rail(false),
        ]
    );
}

#[test]
fn pitch_failure_with_failed_shutdown_reports_both() {
    let mut rail = FakeRail::new();
    rail.write_failures = 1;
    rail.fail_rail_on_call = Some(1);
    assert_eq!(
        block_on(start_note(&mut rail, 1000)),
        Err(BuzzerError::PitchWriteThenShutdown {
            pitch: Fault,
            shutdown: Fault,
        })
    );
    assert_eq!(
        rail.events,
        std::vec![
            RailEvent::Rail(true),
            RailEvent::Wait(STARTUP_WAIT_MS),
            RailEvent::Write(62),
            RailEvent::Rail(false),
        ]
    );
}

#[test]
fn retune_changes_pitch_without_touching_power() {
    let mut rail = FakeRail::new();
    assert_eq!(block_on(retune(&mut rail, 1000)), Ok(62));
    assert_eq!(rail.events, std::vec![RailEvent::Write(62)]);
}

#[test]
fn retune_failure_shuts_down_and_reports_original() {
    let mut rail = FakeRail::new();
    rail.write_failures = 1;
    assert_eq!(
        block_on(retune(&mut rail, 1000)),
        Err(BuzzerError::PitchWrite(Fault))
    );
    assert_eq!(
        rail.events,
        std::vec![RailEvent::Write(62), RailEvent::Rail(false)]
    );
}

#[test]
fn retune_failure_with_failed_shutdown_reports_both() {
    let mut rail = FakeRail::new();
    rail.write_failures = 1;
    rail.fail_rail_on_call = Some(0);
    assert_eq!(
        block_on(retune(&mut rail, 1000)),
        Err(BuzzerError::PitchWriteThenShutdown {
            pitch: Fault,
            shutdown: Fault,
        })
    );
    assert_eq!(
        rail.events,
        std::vec![RailEvent::Write(62), RailEvent::Rail(false)]
    );
}

#[test]
fn end_note_failure_is_reported_not_swallowed() {
    let mut rail = FakeRail::new();
    assert!(block_on(end_note(&mut rail)).is_ok());
    assert_eq!(rail.events, std::vec![RailEvent::Rail(false)]);
    rail.events.clear();
    rail.fail_rail_on_call = Some(1);
    assert_eq!(
        block_on(end_note(&mut rail)),
        Err(BuzzerError::RailShutdown(Fault))
    );
}
