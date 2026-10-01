//! Host proof that asset-retry accounting treats the single-outstanding
//! `Busy` as expected pending state, not failure.
//!
//! Drives two real product pieces together:
//! - the actual SD loader queue (`request_assets` / `take_assets` /
//!   `complete_asset_read` from
//!   `products/meditamer/src/firmware/storage/clock_assets.rs`), whose
//!   `Busy`-while-outstanding contract genuinely produces the poll
//!   sequence below (each mid-flight attempt is asserted to answer
//!   `Busy`, not simulated), and
//! - the real `AssetRetryPolicy` from the analog-clock model
//!   (`products/meditamer/src/firmware/ui/screen/analog_clock/model.rs`),
//!   the exact type the renderer owns and calls from `poll_assets`.
//!
//! The per-tick stepping helper follows the renderer's `poll_assets`
//! mapping (reap a completion when present, else queue while idle with
//! `Busy` counting nothing); the renderer owns that mapping and this file
//! pins the sequence it must produce. The fixed behavior: an accepted
//! request followed by many `Busy` polls and then a successful reply never
//! exhausts the budget (three ordinary ticks during a 2.5 MB SD read used
//! to park the screen before the transfer completed), while genuine read
//! failures still park after exactly `MAX_ATTEMPTS`.
//!
//! No PSRAM claim is made here: the host `LargeByteBuffer` stub only
//! proves channel ownership/drop flow, and dropping a reaped success
//! stands in for the boot-cache adopt the device performs.

extern crate alloc;

pub mod firmware;

use firmware::{
    psram::LargeByteBuffer,
    storage::clock_assets::{self, AssetReadError},
};
use ui_shell_host_harness::analog_clock_model::{AssetQueueAttempt, AssetRetryPolicy};

fn drain() {
    clock_assets::reset_loader_for_tests();
}

/// One renderer-style poll step: reap a completion when present, else
/// attempt a fresh queue. Returns whether the policy parks after it.
fn poll(policy: &mut AssetRetryPolicy) -> bool {
    match clock_assets::take_assets() {
        Some(completion) => match completion.result {
            Ok(buffer) => {
                // Host has no boot cache: dropping returns ownership, which
                // is the property under test (no leak, loader released).
                drop(buffer);
                policy.note_assets_ready();
                false
            }
            Err(_) => policy.note_completion_error(),
        },
        None => match clock_assets::request_assets() {
            Ok(_) => policy.note_queue_attempt(AssetQueueAttempt::Accepted),
            Err(AssetReadError::Busy) => policy.note_queue_attempt(AssetQueueAttempt::Busy),
            Err(_) => policy.note_queue_attempt(AssetQueueAttempt::Refused),
        },
    }
}

#[test]
fn accepted_then_many_busy_then_success_never_parks() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();
    let mut policy = AssetRetryPolicy::new();

    // Tick 1 queues the read: the loader accepts it, and the SD worker
    // receives it (`poll_asset_request` stands in for the worker's
    // `receive_asset_request`; without it the host request channel would
    // stay full and later queues would read `QueueFull`, which is a host
    // artifact, not device behavior).
    assert!(!poll(&mut policy));
    assert_eq!(policy.failures(), 0);
    let queued = clock_assets::poll_asset_request().expect("request queued");

    // The 2.5 MB transfer takes many display ticks. Every further queue
    // attempt while it is in flight must genuinely answer Busy -- and none
    // may consume the retry budget. Poll well past the parking bound.
    for _ in 0..AssetRetryPolicy::MAX_ATTEMPTS + 7 {
        assert!(clock_assets::take_assets().is_none());
        assert_eq!(clock_assets::request_assets(), Err(AssetReadError::Busy));
        assert!(!poll(&mut policy));
    }
    assert_eq!(policy.failures(), 0);
    assert!(!policy.is_parked());

    // The transfer completes successfully; reaping it clears any streak.
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: queued.id,
        generation: queued.generation,
        result: Ok(clock_assets::AssetBuffers::new(core::array::from_fn(|i| {
            LargeByteBuffer::from_vec(if i == 0 {
                alloc::vec![7u8; 8]
            } else {
                alloc::vec![]
            })
        }))),
    });
    assert!(!poll(&mut policy));
    assert_eq!(policy.failures(), 0);
    assert!(!policy.is_parked());

    // Loader released: a fresh request is accepted, proving no wedge.
    assert!(clock_assets::request_assets().is_ok());
    drain();
}

#[test]
fn completion_errors_park_after_bound() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();
    let mut policy = AssetRetryPolicy::new();

    for failure in 1..=AssetRetryPolicy::MAX_ATTEMPTS {
        assert!(!poll(&mut policy));
        let queued = clock_assets::poll_asset_request().expect("request queued");
        for _ in 0..2 {
            assert!(!poll(&mut policy));
        }
        assert_eq!(policy.failures(), failure - 1);
        clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
            id: queued.id,
            generation: queued.generation,
            result: Err(AssetReadError::Unavailable),
        });
        let parked = poll(&mut policy);
        assert_eq!(parked, failure == AssetRetryPolicy::MAX_ATTEMPTS);
        assert_eq!(policy.failures(), failure);
    }
    assert!(policy.is_parked());
    drain();
}

#[test]
fn busy_between_failures_counts_nothing() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();
    let mut policy = AssetRetryPolicy::new();

    // Two genuine failures separated by long Busy stretches: only the two
    // failures count, so the screen must still be unparked.
    for _ in 0..2 {
        assert!(!poll(&mut policy));
        let queued = clock_assets::poll_asset_request().expect("request queued");
        for _ in 0..5 {
            assert!(!poll(&mut policy));
        }
        clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
            id: queued.id,
            generation: queued.generation,
            result: Err(AssetReadError::NotFound),
        });
        assert!(!poll(&mut policy));
    }
    assert_eq!(policy.failures(), 2);
    assert!(!policy.is_parked());

    // The third genuine failure parks, however much Busy came before.
    for _ in 0..5 {
        assert!(!poll(&mut policy));
    }
    // No completion outstanding: polls above only queued/polled Busy.
    // Force the third failure through a fresh completion cycle.
    assert!(clock_assets::take_assets().is_none());
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 61,
        generation: clock_assets::upload_generation(),
        result: Err(AssetReadError::NotFound),
    });
    assert!(poll(&mut policy));
    assert!(policy.is_parked());
    drain();
}

// Pure-policy checks below: no shared queue state, no lock needed. They pin
// the `Refused` arm, which the public queue API only reaches defensively
// (its channel-full path), and the success-reset rule.

#[test]
fn accepted_and_busy_count_nothing() {
    let mut policy = AssetRetryPolicy::new();
    assert!(!policy.note_queue_attempt(AssetQueueAttempt::Accepted));
    for _ in 0..10 {
        assert!(!policy.note_queue_attempt(AssetQueueAttempt::Busy));
    }
    assert_eq!(policy.failures(), 0);
    assert!(!policy.is_parked());
}

#[test]
fn refused_attempts_park_after_bound() {
    let mut policy = AssetRetryPolicy::new();
    for failure in 1..AssetRetryPolicy::MAX_ATTEMPTS {
        assert!(!policy.note_queue_attempt(AssetQueueAttempt::Refused));
        assert_eq!(policy.failures(), failure);
    }
    assert!(policy.note_queue_attempt(AssetQueueAttempt::Refused));
    assert!(policy.is_parked());
}

#[test]
fn success_resets_consecutive_streak() {
    let mut policy = AssetRetryPolicy::new();
    assert!(!policy.note_completion_error());
    assert!(!policy.note_completion_error());
    policy.note_assets_ready();
    assert_eq!(policy.failures(), 0);
    assert!(!policy.note_completion_error());
    assert!(!policy.note_completion_error());
    assert!(!policy.is_parked());
    assert!(policy.note_completion_error());
    assert!(policy.is_parked());
}
