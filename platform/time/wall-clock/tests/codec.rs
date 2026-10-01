//! Round-trip encode/decode for all three message kinds, plus malformed-line
//! rejection.
//!
//! Test binaries for a `no_std` lib crate are ordinary std binaries, so
//! `String` (which implements `core::fmt::Write`) is a fine encode target
//! here even though the crate's real callers write into a fixed buffer
//! on-device (`heapless::String`, matching the rest of the serial wire
//! protocol).

use wall_clock::codec::{
    decode_reply, decode_request, decode_result, encode_reply, encode_request, encode_result,
};
use wall_clock::message::{
    SyncReply, SyncRequest, SyncResult, SyncStatus, TriggerReason, VerifyErrorReason,
    PROTOCOL_VERSION,
};

#[test]
fn request_round_trips_through_encode_and_decode() {
    let request = SyncRequest {
        version: PROTOCOL_VERSION,
        session: 42,
        nonce: 123_456,
        trigger: TriggerReason::ColdBootOffer,
        deadline_ms: 15_000,
    };
    let mut line = String::new();
    encode_request(&request, &mut line).expect("encoding must not fail");
    assert_eq!(
        line,
        "TIME_REQUEST version=1 session=42 nonce=123456 reason=cold_boot_offer deadline_ms=15000"
    );
    assert_eq!(decode_request(&line), Some(request));
}

#[test]
fn reply_round_trips_through_encode_and_decode() {
    let reply = SyncReply {
        version: PROTOCOL_VERSION,
        session: 42,
        nonce: 123_456,
        utc_epoch_seconds: 1_762_531_200,
        offset_minutes: -300,
    };
    let mut line = String::new();
    encode_reply(&reply, &mut line).expect("encoding must not fail");
    assert_eq!(
        line,
        "TIME_REPLY version=1 session=42 nonce=123456 utc=1762531200 offset_min=-300"
    );
    assert_eq!(decode_reply(&line), Some(reply));
}

#[test]
fn ok_result_round_trips_through_encode_and_decode() {
    let result = SyncResult {
        version: PROTOCOL_VERSION,
        session: 42,
        status: SyncStatus::Ok,
        verified: Some((1_762_531_201, -300)),
        reason: None,
    };
    let mut line = String::new();
    encode_result(&result, &mut line).expect("encoding must not fail");
    assert_eq!(
        line,
        "TIME_SYNC OK version=1 session=42 utc=1762531201 offset_min=-300"
    );
    assert_eq!(decode_result(&line), Some(result));
}

#[test]
fn err_result_round_trips_through_encode_and_decode() {
    let result = SyncResult {
        version: PROTOCOL_VERSION,
        session: 42,
        status: SyncStatus::Err,
        verified: None,
        reason: Some(VerifyErrorReason::NotAdvanced),
    };
    let mut line = String::new();
    encode_result(&result, &mut line).expect("encoding must not fail");
    assert_eq!(
        line,
        "TIME_SYNC ERR version=1 session=42 reason=not_advanced"
    );
    assert_eq!(decode_result(&line), Some(result));
}

#[test]
fn an_rtc_driver_reason_round_trips_through_a_sync_result() {
    let result = SyncResult {
        version: PROTOCOL_VERSION,
        session: 1,
        status: SyncStatus::Err,
        verified: None,
        reason: Some(VerifyErrorReason::Rtc("clock_stopped")),
    };
    let mut line = String::new();
    encode_result(&result, &mut line).expect("encoding must not fail");
    assert_eq!(decode_result(&line), Some(result));
}

#[test]
fn decoders_reject_malformed_or_mistagged_lines() {
    assert_eq!(decode_request("TIME_REQUEST version=1 session=1"), None);
    assert_eq!(decode_request("garbage"), None);
    assert_eq!(
        decode_reply("TIME_REQUEST version=1 session=1 nonce=1 reason=manual_demand deadline_ms=1"),
        None
    );
    assert_eq!(
        decode_reply("TIME_REPLY version=1 session=1 nonce=1 utc=not_a_number offset_min=0"),
        None
    );
    assert_eq!(decode_result("TIME_SYNC MAYBE version=1 session=1"), None);
    assert_eq!(
        decode_result("TIME_SYNC OK version=1 session=1 utc=1"),
        None
    );
}

#[test]
fn decoders_reject_trailing_or_interleaved_garbage_after_every_known_field() {
    // Every well-formed field is present; the only problem is one extra
    // token -- exactly what a concurrent writer interleaving bytes into the
    // same line (a documented risk on a shared UART) would produce.
    assert_eq!(
        decode_request(
            "TIME_REQUEST version=1 session=1 nonce=1 reason=manual_demand deadline_ms=1 garbage"
        ),
        None
    );
    assert_eq!(
        decode_reply("TIME_REPLY version=1 session=1 nonce=1 utc=100 offset_min=0 garbage"),
        None
    );
    assert_eq!(
        decode_result("TIME_SYNC OK version=1 session=1 utc=100 offset_min=0 garbage"),
        None
    );
    assert_eq!(
        decode_result("TIME_SYNC ERR version=1 session=1 reason=timeout garbage"),
        None
    );
    // A duplicated field is the same shape of problem: one token too many.
    assert_eq!(
        decode_reply("TIME_REPLY version=1 session=1 session=2 nonce=1 utc=100 offset_min=0"),
        None
    );
}

#[test]
fn decoders_tolerate_extra_leading_and_trailing_whitespace() {
    let reply = SyncReply {
        version: PROTOCOL_VERSION,
        session: 1,
        nonce: 1,
        utc_epoch_seconds: 100,
        offset_minutes: 0,
    };
    assert_eq!(
        decode_reply("  TIME_REPLY version=1 session=1 nonce=1 utc=100 offset_min=0  \r\n"),
        Some(reply)
    );
}
