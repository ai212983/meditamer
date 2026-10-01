//! Host-executable intake tests for the real Ambient Home SD pack loader.
//!
//! Drives the actual queue functions of the unmodified product module
//! (`crate::firmware::storage::ambient_assets`, loaded from
//! `products/meditamer/src/firmware/storage/ambient_assets.rs`). The
//! `LargeByteBuffer` stub (`firmware::psram`) is an owned host `Vec`
//! wrapper proving channel aliasing and drop/ownership behavior, not
//! device PSRAM allocation.

extern crate alloc;

pub mod firmware;

use firmware::{
    psram::LargeByteBuffer,
    storage::{
        ambient_assets::{self, AssetReadError, ASSET_PATH, MAX_FILE_LEN, SKY_LEN},
        clock_assets,
    },
};

fn drain() {
    // Both loaders share the stub but own separate statics; reset both so
    // no binary that also drives the clock queue observes our leftovers
    // (and the clock helper stays strict-but-used here).
    ambient_assets::reset_loader_for_tests();
    clock_assets::reset_loader_for_tests();
}

/// Hold both loader locks, clock first: this binary also compiles the
/// clock module's nested queue tests, which drain manually without taking
/// a lock, so our drains must not interleave with them (same exposure the
/// clock binary already lives with, narrowed to our critical sections).
fn lock() -> (
    std::sync::MutexGuard<'static, ()>,
    std::sync::MutexGuard<'static, ()>,
) {
    let clock = clock_assets::QUEUE_LOCK.lock().unwrap();
    let ambient = ambient_assets::QUEUE_LOCK.lock().unwrap();
    (clock, ambient)
}

#[test]
fn queue_lifecycle_busy_until_taken() {
    let _guard = lock();
    drain();

    let generation = ambient_assets::upload_generation();
    let id = ambient_assets::request_assets().expect("request must queue");
    assert_ne!(id, 0);
    assert_eq!(ambient_assets::request_assets(), Err(AssetReadError::Busy));
    let queued = ambient_assets::poll_asset_request().expect("request must be queued");
    assert_eq!(queued.id, id);
    assert_eq!(queued.generation, generation);
    assert!(ambient_assets::poll_asset_request().is_none());
    assert!(ambient_assets::take_assets().is_none());
    assert_eq!(ambient_assets::request_assets(), Err(AssetReadError::Busy));

    // Delivered completion keeps OUTSTANDING set until taken, echoes the
    // queued id and generation, and carries the landed length the UI needs
    // to borrow the exact pack slice.
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: queued.id,
        generation: queued.generation,
        result: Err(AssetReadError::Unavailable),
        len: 0,
    });
    assert_eq!(ambient_assets::request_assets(), Err(AssetReadError::Busy));
    let taken = ambient_assets::take_assets().expect("completion present");
    assert_eq!(taken.id, queued.id);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::Unavailable));
    assert!(ambient_assets::request_assets().is_ok());

    drain();
}

#[test]
fn upload_generation_captured_per_request() {
    let _guard = lock();
    drain();

    let before = ambient_assets::upload_generation();
    assert!(ambient_assets::request_assets().is_ok());
    let first = ambient_assets::poll_asset_request().expect("first request");
    assert_eq!(first.generation, before);
    ambient_assets::note_committed_upload();
    let after = ambient_assets::upload_generation();
    assert_eq!(after, before.wrapping_add(1));
    assert_ne!(first.generation, after);
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: first.id,
        generation: first.generation,
        result: Err(AssetReadError::Busy),
        len: 0,
    });
    let taken = ambient_assets::take_assets().expect("completion present");
    assert_eq!(taken.generation, before);
    assert!(ambient_assets::request_assets().is_ok());
    let second = ambient_assets::poll_asset_request().expect("second request");
    assert_eq!(second.generation, after);

    drain();
}

#[test]
fn stale_completion_replaced_and_fresh_delivered() {
    let _guard = lock();
    drain();

    let generation = ambient_assets::upload_generation();
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: 7,
        generation,
        result: Err(AssetReadError::Unavailable),
        len: 0,
    });
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: 8,
        generation,
        result: Err(AssetReadError::NotFound),
        len: 0,
    });
    let taken = ambient_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 8);
    assert_eq!(taken.generation, generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));
    assert!(ambient_assets::take_assets().is_none());
    assert!(ambient_assets::request_assets().is_ok());

    drain();
}

#[test]
fn owned_buffer_drops_release_loader() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let _guard = lock();
    drain();
    let drops = Arc::new(AtomicUsize::new(0));

    let generation = ambient_assets::upload_generation();
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: 11,
        generation,
        result: Ok(LargeByteBuffer::counting(alloc::vec![0u8; 8], &drops)),
        len: 8,
    });
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: 12,
        generation,
        result: Err(AssetReadError::NotFound),
        len: 0,
    });
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let taken = ambient_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 12);

    assert!(ambient_assets::request_assets().is_ok());
    assert!(ambient_assets::poll_asset_request().is_some());
    ambient_assets::complete_asset_read(ambient_assets::AmbientReadCompletion {
        id: 13,
        generation,
        result: Ok(LargeByteBuffer::counting(alloc::vec![1u8; 8], &drops)),
        len: 8,
    });
    let taken = ambient_assets::take_assets().expect("owned completion present");
    assert_eq!(taken.id, 13);
    assert!(taken.result.is_ok());
    drop(taken);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert!(ambient_assets::request_assets().is_ok());

    drain();
}

// Pure format vectors: no shared queue state.
#[test]
fn validate_format_vectors() {
    assert_eq!(ASSET_PATH, b"/assets/AMBIENT/SKY.BIN");
    assert_eq!(SKY_LEN, 45_000);
    // Too short for a header.
    assert_eq!(
        ambient_assets::validate_assets(&[0u8; 16]),
        Err(AssetReadError::BadSize)
    );
    // Zeroed max-size file: wrong magic first.
    let file = alloc::vec![0u8; MAX_FILE_LEN];
    assert_eq!(
        ambient_assets::validate_assets(&file),
        Err(AssetReadError::BadMagic)
    );
    // Minimal valid geometry (8x8 sun) with a wrong CRC, then fixed.
    let header_len = ambient_assets::HEADER_LEN;
    let sun_len = 8usize;
    let payload_len = SKY_LEN + 2 * sun_len;
    let mut small = alloc::vec![0u8; header_len + payload_len];
    small[0..8].copy_from_slice(&ambient_assets::MAGIC);
    small[8..10].copy_from_slice(&600u16.to_le_bytes());
    small[10..12].copy_from_slice(&600u16.to_le_bytes());
    small[12..14].copy_from_slice(&8u16.to_le_bytes());
    small[14..16].copy_from_slice(&8u16.to_le_bytes());
    small[16..20].copy_from_slice(&4.0f32.to_le_bytes());
    small[20..24].copy_from_slice(&4.0f32.to_le_bytes());
    small[24..28].copy_from_slice(&(SKY_LEN as u32).to_le_bytes());
    small[28..32].copy_from_slice(&(sun_len as u32).to_le_bytes());
    assert_eq!(
        ambient_assets::validate_assets(&small),
        Err(AssetReadError::BadChecksum)
    );
    let sum = ambient_assets::crc32(&small[header_len..]);
    small[32..36].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(ambient_assets::validate_assets(&small), Ok(()));
    small[40] = 1;
    assert_eq!(
        ambient_assets::validate_assets(&small),
        Err(AssetReadError::BadHeader)
    );
}
