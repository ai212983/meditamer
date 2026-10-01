//! Host-executable intake tests for the real SD asset loader.
//!
//! Drives the actual queue functions of the unmodified product module
//! (`crate::firmware::storage::clock_assets`, loaded from
//! `products/meditamer/src/firmware/storage/clock_assets.rs`). The
//! `LargeByteBuffer` stub (`firmware::psram`) is an owned host `Vec`
//! wrapper proving channel aliasing and drop/ownership behavior, not
//! device PSRAM allocation.

extern crate alloc;

pub mod firmware;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use firmware::{
    psram::LargeByteBuffer,
    storage::clock_assets::{
        self, AssetReadError, ASSET_PATH, FILE_LEN, HEADER_LEN, MAGIC, PAYLOAD_LEN,
    },
};

fn drain() {
    // Full reset, not just channel drains: a passing queue test ends with
    // OUTSTANDING held (its final request proves reusability), and taking
    // nothing leaves the guard set, so drain-only cleanup is
    // order-dependent. The helper also drops any queued buffer.
    clock_assets::reset_loader_for_tests();
}

#[test]
fn queue_busy_until_taken() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();

    assert!(clock_assets::request_assets().is_ok());
    assert_eq!(clock_assets::request_assets(), Err(AssetReadError::Busy));
    let queued = clock_assets::poll_asset_request().expect("request queued");
    assert!(clock_assets::poll_asset_request().is_none());
    assert!(clock_assets::take_assets().is_none());
    assert_eq!(clock_assets::request_assets(), Err(AssetReadError::Busy));

    // Delivered completion keeps OUTSTANDING set until taken.
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: queued.id,
        generation: queued.generation,
        result: Err(AssetReadError::Unavailable),
    });
    assert_eq!(clock_assets::request_assets(), Err(AssetReadError::Busy));
    let taken = clock_assets::take_assets().expect("completion present");
    assert_eq!(taken.id, queued.id);
    assert_eq!(taken.generation, queued.generation);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::Unavailable));
    assert!(clock_assets::request_assets().is_ok());

    drain();
}

#[test]
fn stale_completion_replaced_and_fresh_delivered() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();

    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 7,
        generation: clock_assets::upload_generation(),
        result: Err(AssetReadError::Unavailable),
    });
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 8,
        generation: clock_assets::upload_generation(),
        result: Err(AssetReadError::NotFound),
    });
    let taken = clock_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 8);
    assert_eq!(taken.generation, clock_assets::upload_generation());
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));
    assert!(clock_assets::take_assets().is_none());
    assert!(clock_assets::request_assets().is_ok());

    drain();
}

#[test]
fn owned_buffer_drops_release_loader() {
    let _guard = clock_assets::QUEUE_LOCK.lock().unwrap();
    drain();
    let drops = Arc::new(AtomicUsize::new(0));

    // Stale owned buffer is freed when the fresh completion replaces it.
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 11,
        generation: clock_assets::upload_generation(),
        result: Ok(clock_assets::AssetBuffers::new(core::array::from_fn(|i| {
            if i == 0 {
                LargeByteBuffer::counting(alloc::vec![0u8; 8], &drops)
            } else {
                LargeByteBuffer::from_vec(alloc::vec![])
            }
        }))),
    });
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 12,
        generation: clock_assets::upload_generation(),
        result: Err(AssetReadError::NotFound),
    });
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let taken = clock_assets::take_assets().expect("fresh completion present");
    assert_eq!(taken.id, 12);
    assert_eq!(taken.result.map(|_| ()), Err(AssetReadError::NotFound));

    // Inactive UI takes a success and drops it: the buffer frees and the
    // loader accepts a new request.
    assert!(clock_assets::request_assets().is_ok());
    assert!(clock_assets::poll_asset_request().is_some());
    clock_assets::complete_asset_read(clock_assets::AssetReadCompletion {
        id: 13,
        generation: clock_assets::upload_generation(),
        result: Ok(clock_assets::AssetBuffers::new(core::array::from_fn(|i| {
            if i == 0 {
                LargeByteBuffer::counting(alloc::vec![1u8; 8], &drops)
            } else {
                LargeByteBuffer::from_vec(alloc::vec![])
            }
        }))),
    });
    let taken = clock_assets::take_assets().expect("owned completion present");
    assert_eq!(taken.id, 13);
    assert!(taken.result.is_ok());
    drop(taken);
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    assert!(clock_assets::request_assets().is_ok());

    drain();
}

// Pure format vectors: no shared queue state, no lock needed.
#[test]
fn validate_format_vectors() {
    assert_eq!(ASSET_PATH, b"/assets/CLOCK/MAPS.BIN");
    let mut file = alloc::vec![0u8; FILE_LEN];
    assert_eq!(
        clock_assets::validate_assets(&file),
        Err(AssetReadError::BadMagic)
    );
    file[0..8].copy_from_slice(&MAGIC);
    assert_eq!(
        clock_assets::validate_assets(&file),
        Err(AssetReadError::BadHeader)
    );
    file[8..12].copy_from_slice(&(PAYLOAD_LEN as u32).to_le_bytes());
    let sum = clock_assets::crc32(&file[HEADER_LEN..]);
    file[12..16].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(clock_assets::validate_assets(&file), Ok(()));
    file[31] = 1;
    assert_eq!(
        clock_assets::validate_assets(&file),
        Err(AssetReadError::BadHeader)
    );
    file[31] = 0;
    file[HEADER_LEN] ^= 0xFF;
    assert_eq!(
        clock_assets::validate_assets(&file),
        Err(AssetReadError::BadChecksum)
    );
    assert_eq!(
        clock_assets::validate_assets(&file[..FILE_LEN - 1]),
        Err(AssetReadError::BadSize)
    );
}
