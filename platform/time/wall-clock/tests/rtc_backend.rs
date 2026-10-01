//! `apply_and_verify` behavior against a scriptable [`FakeRtcBackend`]:
//! success, every RTC-driver failure path, the not-advanced/offset-mismatch
//! checks, and that the delayed readback actually waits
//! [`wall_clock::rtc_backend::VERIFY_DELAY_MS`] before re-reading.

mod support;

use rtc::driver::{RtcError, TimeSetOutcome, UnavailableReason};
use support::{block_on, FakeDelay, FakeI2cError, FakeRtcBackend};
use wall_clock::message::{SyncReply, SyncStatus, VerifyErrorReason, PROTOCOL_VERSION};
use wall_clock::rtc_backend::{apply_and_verify, VERIFY_DELAY_MS};
use wall_clock::session::{Coordinator, ReplyOutcome, SourceId, SourceSet};

const SOURCE: SourceId = SourceId(0);

/// Opens a session, accepts one reply for it, and returns that reply --
/// the common setup every `apply_and_verify` test needs before it can call
/// the function under test (which requires an already-`Accepted` reply).
fn open_and_accept(coordinator: &mut Coordinator, utc: u32, offset: i16) -> SyncReply {
    let request = coordinator.open_session(
        wall_clock::message::TriggerReason::ManualDemand,
        1,
        0,
        10_000,
        SourceSet::single(SOURCE),
    );
    let reply = SyncReply {
        version: PROTOCOL_VERSION,
        session: request.session,
        nonce: request.nonce,
        utc_epoch_seconds: utc,
        offset_minutes: offset,
    };
    assert_eq!(
        coordinator.on_reply(SOURCE, &reply, 0),
        ReplyOutcome::Accepted
    );
    reply
}

#[test]
fn succeeds_when_the_delayed_readback_advances_and_matches() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Ok(TimeSetOutcome {
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }));
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::valid_snapshot(1_001, 60)));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Ok);
    assert_eq!(result.verified, Some((1_001, 60)));
    assert_eq!(backend.time_set_calls, std::vec![(1_000, 60)]);
    assert_eq!(backend.read_snapshot_calls, 1);
    assert_eq!(
        delay.delays_ns,
        std::vec![VERIFY_DELAY_MS as u64 * 1_000_000]
    );
    assert!(coordinator.is_closed());
}

#[test]
fn reports_the_rtc_error_when_time_set_itself_fails() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Err(RtcError::I2c(FakeI2cError)));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Rtc("i2c")));
    // A failed write never reaches the delayed verification step.
    assert_eq!(delay.delays_ns, std::vec::Vec::<u64>::new());
    assert_eq!(backend.read_snapshot_calls, 0);
}

#[test]
fn reports_the_rtc_error_when_the_delayed_readback_transaction_fails() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Ok(TimeSetOutcome {
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }));
    backend.read_snapshot_result = Some(Err(RtcError::I2c(FakeI2cError)));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Rtc("i2c")));
}

#[test]
fn reports_the_unavailable_reason_when_the_readback_is_invalid() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Ok(TimeSetOutcome {
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }));
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::unavailable_snapshot(
        UnavailableReason::ClockStopped,
    )));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Rtc("clock_stopped")));
}

#[test]
fn reports_not_advanced_when_the_readback_utc_did_not_move_past_the_reply() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Ok(TimeSetOutcome {
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }));
    // Same value as the reply -- not an advance.
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::valid_snapshot(1_000, 60)));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::NotAdvanced));
}

#[test]
fn reports_offset_mismatch_when_the_readback_offset_disagrees() {
    let mut coordinator = Coordinator::new();
    let reply = open_and_accept(&mut coordinator, 1_000, 60);
    let mut backend = FakeRtcBackend::new();
    backend.time_set_result = Some(Ok(TimeSetOutcome {
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }));
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::valid_snapshot(1_001, 120)));
    let mut delay = FakeDelay::new();

    let result = block_on(apply_and_verify(
        &mut coordinator,
        &mut backend,
        &reply,
        &mut delay,
    ));

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::OffsetMismatch));
}
