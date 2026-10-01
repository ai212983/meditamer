//! `Coordinator` session admission: nonzero-nonce enforcement, deadline/
//! timeout, source-not-allowed, duplicate/collision, stale (wrong session or
//! nonce), and wrong-state replies -- each asserted against `SessionStats`.

use wall_clock::message::{
    SyncReply, SyncStatus, TriggerReason, VerifyErrorReason, PROTOCOL_VERSION,
};
use wall_clock::session::{Coordinator, IgnoreReason, ReplyOutcome, SourceId, SourceSet};

const SOURCE_A: SourceId = SourceId(0);
const SOURCE_B: SourceId = SourceId(1);

#[test]
fn next_timeout_tracks_only_unanswered_sessions_across_wrap() {
    let mut coordinator = Coordinator::new();
    assert_eq!(coordinator.timeout_remaining_ms(0), None);
    let opened = coordinator.open_session(
        TriggerReason::ColdBootOffer,
        7,
        u32::MAX - 9,
        20,
        SourceSet::single(SOURCE_A),
    );
    assert_eq!(coordinator.timeout_remaining_ms(u32::MAX - 9), Some(20));
    assert_eq!(coordinator.timeout_remaining_ms(0), Some(10));
    assert_eq!(coordinator.timeout_remaining_ms(10), Some(0));
    assert_eq!(coordinator.timeout_remaining_ms(11), Some(0));
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &valid_reply(opened.session, 7), 1),
        ReplyOutcome::Accepted
    );
    assert_eq!(coordinator.timeout_remaining_ms(1), None);
}

fn valid_reply(session: u32, nonce: u32) -> SyncReply {
    SyncReply {
        version: PROTOCOL_VERSION,
        session,
        nonce,
        utc_epoch_seconds: 1_000,
        offset_minutes: 60,
    }
}

#[test]
fn a_reply_with_no_open_session_is_ignored_as_wrong_state() {
    let mut coordinator = Coordinator::new();
    let reply = valid_reply(1, 1);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 0),
        ReplyOutcome::Ignored(IgnoreReason::WrongState)
    );
    assert_eq!(coordinator.stats().wrong_state, 1);
}

#[test]
fn the_first_valid_reply_from_an_allowed_source_is_accepted() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        7,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session, request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 10),
        ReplyOutcome::Accepted
    );
}

#[test]
fn a_second_reply_for_an_already_claimed_session_is_a_collision() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        7,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session, request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 10),
        ReplyOutcome::Accepted
    );
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 11),
        ReplyOutcome::Ignored(IgnoreReason::AlreadyClaimed)
    );
    assert_eq!(coordinator.stats().already_claimed, 1);
}

#[test]
fn a_reply_from_a_source_not_named_in_the_session_is_rejected() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        7,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session, request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_B, &reply, 10),
        ReplyOutcome::Ignored(IgnoreReason::SourceNotAllowed)
    );
    assert_eq!(coordinator.stats().source_not_allowed, 1);
}

#[test]
fn a_reply_for_a_different_session_id_is_stale() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        7,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session.wrapping_add(1), request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 10),
        ReplyOutcome::Ignored(IgnoreReason::SessionMismatch)
    );
    assert_eq!(coordinator.stats().session_mismatch, 1);
}

#[test]
fn a_reply_with_the_wrong_nonce_is_stale() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ManualDemand,
        7,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session, request.nonce.wrapping_add(1));
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 10),
        ReplyOutcome::Ignored(IgnoreReason::NonceMismatch)
    );
    assert_eq!(coordinator.stats().nonce_mismatch, 1);
}

#[test]
fn a_reply_arriving_at_or_after_the_deadline_is_expired_and_closes_the_session() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::InvalidColdBoot,
        7,
        1_000,
        5_000,
        SourceSet::single(SOURCE_A),
    );
    let reply = valid_reply(request.session, request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 6_000),
        ReplyOutcome::Ignored(IgnoreReason::Expired)
    );
    assert_eq!(coordinator.stats().expired, 1);
    assert!(coordinator.is_closed());
}

#[test]
fn poll_timeout_closes_an_unanswered_session_past_its_deadline() {
    let mut coordinator = Coordinator::new();
    coordinator.open_session(
        TriggerReason::ColdBootOffer,
        3,
        0,
        1_000,
        SourceSet::single(SOURCE_A),
    );
    assert_eq!(coordinator.poll_timeout(500), None);
    assert!(!coordinator.is_closed());

    let result = coordinator
        .poll_timeout(1_000)
        .expect("session has timed out");
    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Timeout));
    assert!(coordinator.is_closed());
}

#[test]
fn deadlines_survive_the_millisecond_clock_wrapping_around_u32_max() {
    let mut coordinator = Coordinator::new();
    let opened_at = u32::MAX - 200;
    coordinator.open_session(
        TriggerReason::ColdBootOffer,
        3,
        opened_at,
        1_000,
        SourceSet::single(SOURCE_A),
    );
    // deadline_ms wrapped past u32::MAX to 799 (opened_at + 1_000 - 2^32).
    // Just before it, still not expired even though the raw counter has
    // wrapped back down to a small number.
    assert_eq!(coordinator.poll_timeout(500), None);
    assert!(!coordinator.is_closed());

    let result = coordinator
        .poll_timeout(799)
        .expect("session has timed out");
    assert_eq!(result.status, SyncStatus::Err);
    assert_eq!(result.reason, Some(VerifyErrorReason::Timeout));
}

#[test]
fn a_late_reply_after_the_session_already_closed_is_wrong_state_not_expired() {
    let mut coordinator = Coordinator::new();
    let request = coordinator.open_session(
        TriggerReason::ColdBootOffer,
        3,
        0,
        1_000,
        SourceSet::single(SOURCE_A),
    );
    coordinator
        .poll_timeout(1_000)
        .expect("session has timed out and is now closed");

    // A reply for the same (now-stale) session/nonce shows up even later --
    // there is no session at all to admit it against any more.
    let reply = valid_reply(request.session, request.nonce);
    assert_eq!(
        coordinator.on_reply(SOURCE_A, &reply, 5_000),
        ReplyOutcome::Ignored(IgnoreReason::WrongState)
    );
    assert_eq!(coordinator.stats().wrong_state, 1);
}

#[test]
#[should_panic(expected = "nonzero")]
fn open_session_rejects_a_zero_nonce() {
    let mut coordinator = Coordinator::new();
    coordinator.open_session(
        TriggerReason::ManualDemand,
        0,
        0,
        5_000,
        SourceSet::single(SOURCE_A),
    );
}

#[test]
fn each_session_gets_a_distinct_monotonic_id() {
    let mut coordinator = Coordinator::new();
    let first = coordinator.open_session(
        TriggerReason::ManualDemand,
        1,
        0,
        1_000,
        SourceSet::single(SOURCE_A),
    );
    coordinator.poll_timeout(1_000);
    let second = coordinator.open_session(
        TriggerReason::ManualDemand,
        1,
        1_000,
        1_000,
        SourceSet::single(SOURCE_A),
    );
    assert_ne!(first.session, second.session);
}
