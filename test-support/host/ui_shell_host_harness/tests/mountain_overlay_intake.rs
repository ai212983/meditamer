//! Host-executable intake tests for the real mountain overlay loader.
//!
//! Drives the actual queue functions of the unmodified product module
//! (`crate::firmware::storage::mountain_assets`, loaded from
//! `products/meditamer/src/firmware/storage/mountain_assets.rs`). The
//! `LargeByteBuffer` stub (`firmware::psram`) is an owned host `Vec`
//! wrapper proving channel aliasing and drop/ownership behavior, not
//! device PSRAM allocation. The overlay-layout test pins the intake's
//! `OVERLAY_*` constants against the ambient composer's frozen planes so
//! the SD worker and the UI cannot drift apart.

extern crate alloc;

pub mod firmware;

use firmware::{
    psram::LargeByteBuffer,
    storage::{
        clock_assets,
        mountain_assets::{
            self, AssetReadError, MOUNTAIN_FILE_LEN, MOUNTAIN_FIRST_ROW, MOUNTAIN_ROWS,
            OVERLAY_LEN, OVERLAY_PLANE_BYTES, OVERLAY_ROW_BYTES,
        },
    },
};
use ui_shell_host_harness::ambient_composer;

fn drain() {
    // Both loaders share the stub but own separate statics; reset both so
    // no binary that also drives the clock queue observes our leftovers
    // (and the clock helper stays strict-but-used here).
    mountain_assets::reset_loader_for_tests();
    clock_assets::reset_loader_for_tests();
}

/// Hold both loader locks, clock first: this binary also compiles the
/// clock module's nested queue tests, which observe the clock statics, so
/// our drains must not interleave with them.
fn lock() -> (
    std::sync::MutexGuard<'static, ()>,
    std::sync::MutexGuard<'static, ()>,
) {
    let clock = clock_assets::QUEUE_LOCK.lock().unwrap();
    let mountain = mountain_assets::QUEUE_LOCK.lock().unwrap();
    (clock, mountain)
}

#[test]
fn queue_lifecycle_percent_generation_busy_until_taken() {
    let _guard = lock();
    drain();

    let generation = mountain_assets::upload_generation();
    assert!(mountain_assets::request_assets(20).is_ok());
    assert_eq!(
        mountain_assets::request_assets(22),
        Err(AssetReadError::Busy)
    );
    let queued = mountain_assets::poll_asset_request().expect("request must be queued");
    assert_eq!(queued.percent, 20);
    assert_eq!(queued.generation, generation);
    assert!(mountain_assets::poll_asset_request().is_none());
    assert!(mountain_assets::take_assets().is_none());
    assert_eq!(
        mountain_assets::request_assets(22),
        Err(AssetReadError::Busy)
    );

    // Delivered completion echoes id/percent/generation and keeps
    // OUTSTANDING set until taken.
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: 1,
        percent: 20,
        generation,
        result: Err(AssetReadError::Unavailable),
    });
    assert_eq!(
        mountain_assets::request_assets(22),
        Err(AssetReadError::Busy)
    );
    let taken = mountain_assets::take_assets().expect("completion present");
    assert_eq!(taken.id, 1);
    assert_eq!(taken.percent, 20);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::Unavailable));
    assert!(mountain_assets::request_assets(22).is_ok());

    drain();
}

#[test]
fn invalid_percent_rejected_before_outstanding() {
    let _guard = lock();
    drain();

    assert_eq!(
        mountain_assets::request_assets(101),
        Err(AssetReadError::BadHeader)
    );
    assert_eq!(
        mountain_assets::request_assets(u8::MAX),
        Err(AssetReadError::BadHeader)
    );
    // The guard was never set: the boundary percents still queue.
    assert!(mountain_assets::request_assets(100).is_ok());
    let queued = mountain_assets::poll_asset_request().expect("valid request must queue");
    assert_eq!(queued.percent, 100);
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: queued.id,
        percent: queued.percent,
        generation: queued.generation,
        result: Err(AssetReadError::Unavailable),
    });
    assert!(mountain_assets::take_assets().is_some());
    assert!(mountain_assets::request_assets(0).is_ok());
    let queued = mountain_assets::poll_asset_request().expect("zero percent must queue");
    assert_eq!(queued.percent, 0);

    drain();
}

#[test]
fn stale_completion_replaced_and_fresh_delivered() {
    let _guard = lock();
    drain();

    let generation = mountain_assets::upload_generation();
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: 7,
        percent: 20,
        generation,
        result: Err(AssetReadError::Unavailable),
    });
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: 8,
        percent: 22,
        generation,
        result: Err(AssetReadError::NotFound),
    });
    let taken = mountain_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 8);
    assert_eq!(taken.percent, 22);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));
    assert!(mountain_assets::take_assets().is_none());
    assert!(mountain_assets::request_assets(22).is_ok());

    drain();
}

#[test]
fn generation_capture_and_staleness_predicates() {
    let _guard = lock();
    drain();

    let before = mountain_assets::upload_generation();
    assert!(mountain_assets::request_assets(10).is_ok());
    let first = mountain_assets::poll_asset_request().expect("first request");
    assert_eq!(first.generation, before);
    assert!(mountain_assets::request_is_current(
        first.generation,
        before
    ));
    // A commit supersedes the queued request; the next queue captures the
    // new generation while the old one reports stale.
    mountain_assets::note_committed_upload();
    let after = mountain_assets::upload_generation();
    assert_eq!(after, before.wrapping_add(1));
    assert!(!mountain_assets::request_is_current(
        first.generation,
        after
    ));

    // The UI's stale-drop decision: only a current-generation completion
    // for the still-desired percent is usable.
    assert!(mountain_assets::completion_is_usable(
        after,
        20,
        after,
        Some(20)
    ));
    assert!(!mountain_assets::completion_is_usable(
        before,
        20,
        after,
        Some(20)
    ));
    assert!(!mountain_assets::completion_is_usable(
        after,
        20,
        after,
        Some(22)
    ));
    assert!(!mountain_assets::completion_is_usable(
        after, 20, after, None
    ));

    // The SD worker's validate-once decision: reuse only when the request
    // is current and the cached session carries its generation.
    assert!(mountain_assets::session_reusable(Some(after), after, after));
    assert!(!mountain_assets::session_reusable(
        Some(before),
        before,
        after
    ));
    assert!(!mountain_assets::session_reusable(None, after, after));
    assert!(!mountain_assets::session_reusable(
        Some(before),
        after,
        after
    ));

    drain();
}

#[test]
fn owned_overlay_buffer_drop_releases_loader() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let _guard = lock();
    drain();
    let drops = Arc::new(AtomicUsize::new(0));
    let generation = mountain_assets::upload_generation();

    // A completion owns exactly one overlay — never the whole 665,839-byte
    // pack — and a stale replacement frees it.
    let overlay = LargeByteBuffer::counting(alloc::vec![0u8; OVERLAY_LEN], &drops);
    assert_ne!(OVERLAY_LEN, MOUNTAIN_FILE_LEN);
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: 11,
        percent: 20,
        generation,
        result: Ok(overlay),
    });
    mountain_assets::complete_asset_read(mountain_assets::MountainReadCompletion {
        id: 12,
        percent: 22,
        generation,
        result: Err(AssetReadError::NotFound),
    });
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let taken = mountain_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 12);

    assert!(mountain_assets::request_assets(22).is_ok());
    assert!(mountain_assets::poll_asset_request().is_some());

    drain();
}

#[test]
fn overlay_layout_matches_the_ambient_composer() {
    assert_eq!(
        OVERLAY_ROW_BYTES,
        ambient_composer::MOUNTAIN_OVERLAY_ROW_BYTES
    );
    assert_eq!(
        OVERLAY_PLANE_BYTES,
        ambient_composer::MOUNTAIN_OVERLAY_PLANE_BYTES
    );
    assert_eq!(OVERLAY_LEN, ambient_composer::MOUNTAIN_OVERLAY_LEN);
    assert_eq!(MOUNTAIN_ROWS, ambient_composer::MOUNTAIN_OVERLAY_ROWS);
    assert_eq!(
        MOUNTAIN_FIRST_ROW,
        ambient_composer::MOUNTAIN_OVERLAY_FIRST_ROW
    );
    assert_eq!(MOUNTAIN_FILE_LEN, 665_839);
    assert_eq!(OVERLAY_LEN, 2 * OVERLAY_PLANE_BYTES);
}
