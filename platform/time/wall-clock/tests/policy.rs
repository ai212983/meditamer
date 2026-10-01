//! Shared boot policy: both board adapters must derive the same trigger from
//! a fresh RTC read, and neither session opening nor timeout may write time.

#[allow(dead_code)]
mod support;

use rtc::driver::{RtcError, UnavailableReason};
use support::{block_on, FakeI2cError, FakeRtcBackend};
use wall_clock::message::{SyncStatus, TriggerReason, VerifyErrorReason};
use wall_clock::policy::open_boot_session;
use wall_clock::session::{Coordinator, SourceId, SourceSet};

const SOURCE: SourceId = SourceId(0);

fn open(backend: &mut FakeRtcBackend) -> (Coordinator, TriggerReason) {
    let mut coordinator = Coordinator::new();
    let started = block_on(open_boot_session(
        &mut coordinator,
        backend,
        1,
        10,
        1_000,
        SourceSet::single(SOURCE),
    ));
    (coordinator, started.request.trigger)
}

#[test]
fn valid_rtc_opens_an_optional_boot_offer_without_writing() {
    let mut backend = FakeRtcBackend::new();
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::valid_snapshot(1_000, 60)));

    let (_coordinator, trigger) = open(&mut backend);

    assert_eq!(trigger, TriggerReason::ColdBootOffer);
    assert_eq!(backend.read_snapshot_calls, 1);
    assert!(backend.time_set_calls.is_empty());
}

#[test]
fn unavailable_rtc_opens_required_provisioning_without_writing() {
    let mut backend = FakeRtcBackend::new();
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::unavailable_snapshot(
        UnavailableReason::ClockStopped,
    )));

    let (_coordinator, trigger) = open(&mut backend);

    assert_eq!(trigger, TriggerReason::InvalidColdBoot);
    assert!(backend.time_set_calls.is_empty());
}

#[test]
fn unreadable_rtc_opens_required_provisioning_without_writing() {
    let mut backend = FakeRtcBackend::new();
    backend.read_snapshot_result = Some(Err(RtcError::I2c(FakeI2cError)));

    let (_coordinator, trigger) = open(&mut backend);

    assert_eq!(trigger, TriggerReason::InvalidColdBoot);
    assert!(backend.time_set_calls.is_empty());
}

#[test]
fn boot_session_timeout_does_not_write_time() {
    let mut backend = FakeRtcBackend::new();
    backend.read_snapshot_result = Some(Ok(FakeRtcBackend::unavailable_snapshot(
        UnavailableReason::ClockStopped,
    )));
    let (mut coordinator, _trigger) = open(&mut backend);

    let result = coordinator.poll_timeout(1_010).expect("expired session");

    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Timeout));
    assert!(backend.time_set_calls.is_empty());
}
