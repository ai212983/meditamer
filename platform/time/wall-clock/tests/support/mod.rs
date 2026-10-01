//! Test doubles for `wall-clock`'s host tests: a no-executor future poller
//! (same trick as `platform/time/rtc/tests/support`), a scriptable
//! [`FakeRtcBackend`], and a delay double that records what it was asked to
//! wait.

use rtc::driver::{RtcError, TimeSetOutcome, UnavailableReason, WallClockSnapshot};
use wall_clock::rtc_backend::RtcBackend;

/// Polls a future to completion. None of these fakes ever actually pend, so
/// a trivial no-op-waker poll loop is all that's needed -- no executor
/// dependency required.
pub fn block_on<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    loop {
        if let core::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct FakeI2cError;

/// A fully scriptable [`RtcBackend`]: each call consults (and consumes,
/// where noted) a queued response so a test can drive exactly the sequence
/// `apply_and_verify` is expected to see.
pub struct FakeRtcBackend {
    pub time_set_result: Option<Result<TimeSetOutcome, RtcError<FakeI2cError>>>,
    pub read_snapshot_result: Option<Result<WallClockSnapshot, RtcError<FakeI2cError>>>,
    pub time_set_calls: std::vec::Vec<(u32, i16)>,
    pub read_snapshot_calls: u32,
}

impl FakeRtcBackend {
    pub fn new() -> Self {
        Self {
            time_set_result: None,
            read_snapshot_result: None,
            time_set_calls: std::vec::Vec::new(),
            read_snapshot_calls: 0,
        }
    }

    pub fn valid_snapshot(utc_epoch_seconds: u32, offset_minutes: i16) -> WallClockSnapshot {
        WallClockSnapshot {
            valid: true,
            utc_epoch_seconds,
            local_epoch_seconds: (utc_epoch_seconds as i64 + offset_minutes as i64 * 60) as u32,
            offset_minutes,
            reason: None,
        }
    }

    pub fn unavailable_snapshot(reason: UnavailableReason) -> WallClockSnapshot {
        WallClockSnapshot {
            valid: false,
            utc_epoch_seconds: 0,
            local_epoch_seconds: 0,
            offset_minutes: 0,
            reason: Some(reason),
        }
    }
}

impl Default for FakeRtcBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RtcBackend for FakeRtcBackend {
    type Error = FakeI2cError;

    async fn time_set(
        &mut self,
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    ) -> Result<TimeSetOutcome, RtcError<Self::Error>> {
        self.time_set_calls
            .push((utc_epoch_seconds, offset_minutes));
        self.time_set_result
            .expect("test must set time_set_result before calling time_set")
    }

    async fn read_snapshot(&mut self) -> Result<WallClockSnapshot, RtcError<Self::Error>> {
        self.read_snapshot_calls += 1;
        self.read_snapshot_result
            .expect("test must set read_snapshot_result before calling read_snapshot")
    }
}

/// A [`embedded_hal_async::delay::DelayNs`] double that never actually
/// sleeps -- these are host tests with no real clock -- but records every
/// requested delay in nanoseconds so a test can assert `apply_and_verify`
/// waited before its delayed readback.
pub struct FakeDelay {
    pub delays_ns: std::vec::Vec<u64>,
}

impl FakeDelay {
    pub fn new() -> Self {
        Self {
            delays_ns: std::vec::Vec::new(),
        }
    }
}

impl Default for FakeDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl embedded_hal_async::delay::DelayNs for FakeDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.delays_ns.push(ns as u64);
    }
}
