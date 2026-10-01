//! Host proof that the real analog-clock [`RenderEngine`] separates base
//! allocation from base readiness.
//!
//! Drives the unmodified product renderer
//! (`products/meditamer/src/firmware/ui/screen/analog_clock/render.rs`,
//! included below as `engine` with its real sibling `model`).
//! Only the ESP-only externs are host shims, kept
//! in this file so the product stays untouched:
//!
//! - `esp_alloc::ExternalMemory` is a fallible heap allocator with the
//!   same contract as the device PSRAM allocator (backed by the host
//!   heap; the `extern crate self as ...` aliases satisfy the product's
//!   absolute extern paths without new crates),
//! - `console::println!` forwards to the host stdout log,
//! - `render::lvgl_adapter::ExternalL8CanvasBuffer` is an opaque retained
//!   canvas the engine never draws through here,
//! - `firmware::psram` owns host bytes with the same strict-PSRAM shape
//!   the engine asserts (`placement() == Psram`, leak-once `'static`),
//! - `firmware::storage::clock_assets` answers "nothing pending", since
//!   these tests seed the boot-lifetime asset bytes directly.
//!
//! The pack fixture is synthetic but schema-valid: it is built from the
//! real `clock-assets` constants (`MAGIC`, offsets, `encode_header` CRC)
//! with a zeroed payload, which the core's own map validation accepts.
//!
//! Dual-cache readiness: the completed stationary base is two buffers —
//! `shared.base_bits` (45 KB packed dial) plus `shared.base_gray`
//! (360 KB retained grayscale dial) — plus `shared.base_complete`. The
//! base pass populates both together; the imported dial shades to
//! all-`Background` regions. Readiness is completion of both, never
//! allocation of either.
//!
//! Regression: `service` used to treat an allocated base buffer as a
//! finished one (`shared.base_bits.is_none()` gate) while the
//! cooperative pass still had rows left, and the base-step guard refused
//! the in-progress `RenderingBase` phase, so the base never finished and
//! no `CLOCK_FRAME` ever completed (blank clock). A retarget arriving
//! mid-pass additionally orphaned the base cursor. The tests below fail
//! on that shape: no frame may complete before the full base does, a
//! partial base must never read as ready (including across a navigation
//! that drops the engine mid-pass), either cache missing or incomplete
//! must read as unready, and successive minutes must render. A final
//! parity test checks the streamed cached frame against the original
//! full-ray shader over a non-transparent pack.

extern crate self as console;
extern crate self as embassy_time;
extern crate self as esp_alloc;
extern crate self as render;

/// Monotonic host stand-in for the renderer's Embassy microsecond clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        let elapsed = ORIGIN.get_or_init(std::time::Instant::now).elapsed();
        Self(u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX))
    }
    pub fn as_micros(&self) -> u64 {
        self.0
    }
}

mod host_alloc {
    use allocator_api2::alloc::{AllocError, Allocator, Global};
    use std::alloc::Layout;
    use std::ptr::NonNull;

    /// Host stand-in for `esp_alloc::ExternalMemory`: the same fallible
    /// allocator contract the product's row scratch is built on, backed
    /// by the host heap instead of PSRAM.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct ExternalMemory;

    // SAFETY: every call delegates to the host global allocator with the
    // caller's layout, exactly as a heap-backed `Allocator` must.
    unsafe impl Allocator for ExternalMemory {
        fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            Global.allocate(layout)
        }

        unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            unsafe { Global.deallocate(ptr, layout) }
        }
    }
}
pub use host_alloc::ExternalMemory;

mod host_log {
    macro_rules! println {
        ($($t:tt)*) => {
            std::println!($($t)*)
        };
    }
    pub(crate) use println;
}
pub(crate) use host_log::println;

/// Host stand-in for `render::lvgl_adapter`: the engine only retains the
/// publish canvas here and never draws through it.
mod lvgl_adapter {
    #[derive(Debug)]
    pub struct ExternalL8CanvasBuffer;

    pub struct PreparedL8DrawUnit;
}

/// Host stand-ins for the firmware services the renderer calls into.
/// Same ownership shapes as the device (`crate::firmware::...` paths
/// inside the product resolve here), host bytes underneath.
pub mod firmware {
    pub mod psram {
        use std::vec::Vec;

        /// Owned host bytes with the strict-PSRAM surface the engine
        /// asserts: always `Psram`, owned until the screen releases it.
        #[derive(Debug)]
        pub struct LargeByteBuffer {
            buf: Vec<u8>,
        }

        impl LargeByteBuffer {
            pub fn as_slice(&self) -> &[u8] {
                &self.buf
            }

            pub fn as_mut_slice(&mut self) -> &mut [u8] {
                &mut self.buf
            }

            pub fn placement(&self) -> BufferPlacement {
                BufferPlacement::Psram
            }

            pub fn into_static_mut_slice(self) -> &'static mut [u8] {
                Box::leak(self.buf.into_boxed_slice())
            }
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum BufferPlacement {
            Psram,
            InternalRam,
        }

        #[derive(Debug)]
        pub struct BufferAllocError;

        /// Fallible host allocation mirroring the device's fallible PSRAM
        /// claim (no stack fallback, no panic on exhaustion).
        pub fn alloc_large_byte_buffer(
            byte_len: usize,
        ) -> Result<LargeByteBuffer, BufferAllocError> {
            let mut buf = Vec::new();
            buf.try_reserve_exact(byte_len)
                .map_err(|_| BufferAllocError)?;
            buf.resize(byte_len, 0);
            Ok(LargeByteBuffer { buf })
        }

        pub fn acquire_runtime_bundle(
            requests: &[phase_arena::PartRequest],
        ) -> Result<
            phase_arena::RuntimeBundle<crate::ExternalMemory>,
            phase_arena::RuntimeBundleError,
        > {
            phase_arena::RuntimeBundle::try_new_zeroed(
                requests,
                3 * 1024 * 1024,
                crate::ExternalMemory,
            )
        }
    }

    pub mod storage {
        pub mod clock_assets {
            use super::super::psram::LargeByteBuffer;
            use std::cell::{Cell, RefCell};

            std::thread_local! {
                static COMPLETION: RefCell<Option<AssetReadCompletion>> = const { RefCell::new(None) };
                static REQUESTS_ENABLED: Cell<bool> = const { Cell::new(false) };
                static NEXT_ID: Cell<u32> = const { Cell::new(1) };
                static LAST_ID: Cell<u32> = const { Cell::new(0) };
                static UPLOAD_GENERATION: Cell<u32> = const { Cell::new(0) };
            }

            pub struct AssetBuffers {
                pub maps: [LargeByteBuffer; 9],
            }

            impl AssetBuffers {
                pub fn borrowed(&self) -> Option<::clock_assets::ClockAssetParts<'_>> {
                    ::clock_assets::ClockAssetParts::borrow_validated(core::array::from_fn(|i| {
                        self.maps[i].as_slice()
                    }))
                    .ok()
                }
            }

            /// Loader surface the engine polls. These tests seed the
            /// boot-lifetime asset bytes directly, so the loader is always
            /// idle; the types still mirror the product contract.
            #[derive(Clone, Copy, Debug, Eq, PartialEq)]
            pub enum AssetReadError {
                Busy,
                Unavailable,
                NotFound,
            }

            pub struct AssetReadCompletion {
                pub id: u32,
                pub generation: u32,
                pub result: Result<AssetBuffers, AssetReadError>,
            }

            pub fn take_assets() -> Option<AssetReadCompletion> {
                COMPLETION.with(|slot| slot.borrow_mut().take())
            }

            pub fn request_assets() -> Result<u32, AssetReadError> {
                if !REQUESTS_ENABLED.with(Cell::get) {
                    return Err(AssetReadError::Busy);
                }
                let id = NEXT_ID.with(|next| {
                    let id = next.get();
                    next.set(id + 1);
                    id
                });
                LAST_ID.with(|last| last.set(id));
                Ok(id)
            }

            pub fn enable_requests(enabled: bool) {
                REQUESTS_ENABLED.with(|flag| flag.set(enabled));
            }

            pub fn last_request_id() -> u32 {
                LAST_ID.with(Cell::get)
            }

            pub fn upload_generation() -> u32 {
                UPLOAD_GENERATION.with(Cell::get)
            }

            pub fn note_committed_upload() {
                UPLOAD_GENERATION.with(|generation| {
                    generation.set(generation.get().wrapping_add(1));
                });
            }

            pub fn complete(completion: AssetReadCompletion) {
                COMPLETION.with(|slot| *slot.borrow_mut() = Some(completion));
            }
        }
    }
}

// The borrowed firmware modules also expose APIs used by the real UI,
// beyond the engine operations exercised by this host test.
#[allow(dead_code)]
#[path = "../../../../products/meditamer/src/firmware/ui/screen/analog_clock/model.rs"]
mod model;

#[allow(dead_code)]
#[path = "../../../../products/meditamer/src/firmware/ui/screen/analog_clock/render.rs"]
mod engine;

use engine::{FrameSettlement, RenderEngine, SharedCache, TargetMinute, PACKED_LEN};

/// Local wall time under test, in seconds since the local epoch.
fn t(hour: u32, minute: u32, second: u32) -> u32 {
    hour * 3_600 + minute * 60 + second
}

fn target_at(local_epoch_seconds: u32, intent: model::MinuteIntent) -> TargetMinute {
    TargetMinute::new(
        model::epoch_minute(local_epoch_seconds),
        local_epoch_seconds,
        intent,
    )
}

fn split_pack(pack: &[u8]) -> firmware::storage::clock_assets::AssetBuffers {
    clock_assets::decode(pack).expect("fixture pack validates");
    let payload = &pack[clock_assets::HEADER_LEN..];
    let make = |bytes: &[u8]| {
        let mut buffer =
            firmware::psram::alloc_large_byte_buffer(bytes.len()).expect("host region");
        buffer.as_mut_slice().copy_from_slice(bytes);
        buffer
    };
    firmware::storage::clock_assets::AssetBuffers {
        maps: core::array::from_fn(|i| {
            make(
                &payload[clock_assets::MAP_OFFSETS[i]
                    ..clock_assets::MAP_OFFSETS[i] + clock_assets::MAP_LENGTHS[i]],
            )
        }),
    }
}

/// Schema-valid synthetic pack: real header (magic, length, CRC) over a
/// zeroed payload. The core accepts zeroed maps (fully transparent
/// hands), which is all a progress test needs.
fn seed_shared() -> SharedCache {
    let payload = std::vec![0u8; clock_assets::PAYLOAD_LEN];
    let header = clock_assets::encode_header(&payload).expect("zero payload encodes");
    let mut pack = std::vec::Vec::with_capacity(clock_assets::FILE_LEN);
    pack.extend_from_slice(&header);
    pack.extend_from_slice(&payload);
    assert_eq!(pack.len(), clock_assets::FILE_LEN);
    let buffers = split_pack(&pack);
    let mut shared = SharedCache::new();
    assert!(!shared.base_ready());
    assert!(shared.base_bits.is_none());
    assert!(shared.base_gray.is_none());
    shared.asset_bytes = Some(buffers);
    shared
}

/// Schema-valid pack with real hand coverage for physics parity: the
/// zero-alpha [`seed_shared`] pack leaves every hand pixel transparent,
/// which cannot distinguish the cached restore path from the full ray
/// shader. This pack keeps exact schema lengths but gives both hands an
/// opaque vertical bar (distinct albedo per hand) over flat facing
/// normals, so hand/shadow footprints are non-empty and the dithered
/// hand pixels must match the oracle bit for bit.
///
/// No real asset is read: no `.pack` is checked in, so the fixture is
/// built in memory from the real `clock-assets` layout constants and
/// header encoder. No local paths involved.
fn seed_textured_shared() -> SharedCache {
    use clock_assets::{
        DIAL_H, DIAL_W, HOUR_H, HOUR_W, MINUTE_H, MINUTE_W, OFF_DIAL, OFF_HOUR_ALBEDO,
        OFF_HOUR_ALPHA, OFF_HOUR_NORMAL, OFF_HOUR_SPEC, OFF_MINUTE_ALBEDO, OFF_MINUTE_ALPHA,
        OFF_MINUTE_NORMAL, OFF_MINUTE_SPEC, PAYLOAD_LEN,
    };
    assert_eq!(DIAL_W * DIAL_H, 360_000);
    let mut payload = std::vec![0u8; PAYLOAD_LEN];
    // Imported dial: flat mid-gray artwork (validates square 600x600).
    payload[OFF_DIAL..OFF_HOUR_ALBEDO].fill(128);
    // Flat facing normals (stored green-up): decode to ~+z after the
    // renderer's green flip.
    for range in [
        OFF_HOUR_NORMAL..OFF_HOUR_SPEC,
        OFF_MINUTE_NORMAL..OFF_MINUTE_SPEC,
    ] {
        for px in payload[range].chunks_exact_mut(3) {
            px[0] = 127;
            px[1] = 127;
            px[2] = 255;
        }
    }
    // Distinct gray albedo per hand (compact one-byte-per-pixel form).
    payload[OFF_HOUR_ALBEDO..OFF_HOUR_ALPHA].fill(200);
    payload[OFF_MINUTE_ALBEDO..OFF_MINUTE_ALPHA].fill(90);
    // Opaque vertical bars: hour sprite x in [100, 145), minute sprite
    // x in [60, 100). Everywhere else stays transparent.
    for (alpha_off, width, height, lo, hi) in [
        (OFF_HOUR_ALPHA, HOUR_W, HOUR_H, 100usize, 145usize),
        (OFF_MINUTE_ALPHA, MINUTE_W, MINUTE_H, 60usize, 100usize),
    ] {
        let pixels = width * height;
        let alpha = &mut payload[alpha_off..alpha_off + pixels];
        for (i, cell) in alpha.iter_mut().enumerate() {
            let x = i % width;
            *cell = u8::from(x >= lo && x < hi) * 255;
        }
    }
    let header = clock_assets::encode_header(&payload).expect("textured payload encodes");
    let mut pack = std::vec::Vec::with_capacity(clock_assets::FILE_LEN);
    pack.extend_from_slice(&header);
    pack.extend_from_slice(&payload);
    assert_eq!(pack.len(), clock_assets::FILE_LEN);
    // Sanity: both bars survived (non-transparent coverage exists).
    assert!(payload[OFF_HOUR_ALPHA..OFF_HOUR_NORMAL]
        .iter()
        .any(|&a| a != 0));
    assert!(payload[OFF_MINUTE_ALPHA..OFF_MINUTE_NORMAL]
        .iter()
        .any(|&a| a != 0));
    let buffers = split_pack(&pack);
    let mut shared = SharedCache::new();
    assert!(!shared.base_ready());
    shared.asset_bytes = Some(buffers);
    shared
}

/// MSB-first packed-bit read over a full-frame bit plane.
fn packed_bit_at(bits: &[u8], x: usize, y: usize, width: usize) -> bool {
    let bit = y * width + x;
    bits[bit / 8] & (0x80 >> (bit % 8)) != 0
}

fn fresh_engine() -> RenderEngine {
    RenderEngine::new().expect("host heap serves the row scratch")
}

#[test]
fn reentry_drops_a_previous_activations_asset_completion() {
    use firmware::storage::clock_assets::{self, AssetReadCompletion};

    clock_assets::enable_requests(true);
    let mut previous = fresh_engine();
    let mut previous_shared = SharedCache::new();
    previous.service(&mut previous_shared);
    let stale_id = clock_assets::last_request_id();
    assert_ne!(stale_id, 0);
    drop(previous);
    drop(previous_shared);

    let mut current = fresh_engine();
    let mut shared = SharedCache::new();
    let stale_buffer = seed_shared().asset_bytes.take().expect("valid pack");
    clock_assets::complete(AssetReadCompletion {
        id: stale_id,
        generation: clock_assets::upload_generation(),
        result: Ok(stale_buffer),
    });
    current.service(&mut shared);
    assert!(
        shared.asset_bytes.is_none(),
        "previous pack must be discarded"
    );

    current.service(&mut shared);
    let current_id = clock_assets::last_request_id();
    assert_ne!(current_id, stale_id);
    let current_buffer = seed_shared().asset_bytes.take().expect("valid pack");
    clock_assets::complete(AssetReadCompletion {
        id: current_id,
        generation: clock_assets::upload_generation(),
        result: Ok(current_buffer),
    });
    current.service(&mut shared);
    assert!(shared.asset_bytes.is_some(), "current pack must be adopted");
    clock_assets::enable_requests(false);
}

/// Tick until the predicate holds or the bound runs out; returns the
/// ticks spent. The bound is generous on purpose: it only guards
/// against a stalled engine, never pins the cooperative budgets.
fn tick_until(
    engine: &mut RenderEngine,
    shared: &mut SharedCache,
    bound: u32,
    mut done: impl FnMut(&RenderEngine, &SharedCache) -> bool,
) -> u32 {
    let mut ticks = 0;
    while !done(engine, shared) {
        assert!(
            ticks < bound,
            "engine stalled: no progress within {bound} ticks"
        );
        engine.service(shared);
        ticks += 1;
    }
    ticks
}

#[test]
fn no_frame_before_complete_base_then_frame() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);

    // The whole base pass must produce no frame: allocation is not
    // readiness, and the old gate treated it as such.
    while !shared.base_ready() {
        assert_eq!(engine.service(&mut shared), None);
        assert!(engine.frame_ready().is_none());
        assert!(engine.failed().is_none());
    }
    // Both caches land together in the same base pass; neither alone is
    // readiness.
    assert_eq!(
        shared
            .base_bits
            .as_ref()
            .and_then(|bits| bits.part(0))
            .map(|part| part.len()),
        Some(PACKED_LEN)
    );
    assert_eq!(
        shared
            .base_gray
            .as_ref()
            .and_then(|gray| gray.part(0))
            .map(|part| part.len()),
        Some(engine::GRAY_CACHE_LEN)
    );

    // With the base genuinely complete, the minute's frame must finish.
    let got = tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert!(got > 0, "frame needs row ticks after the base");
    assert_eq!(engine.frame_ready(), Some(target));
    assert_eq!(
        engine.staging_bits().map(|bits| bits.len()),
        Some(PACKED_LEN)
    );
    assert!(engine.failed().is_none());
}

#[test]
fn navigation_releases_clock_pack_and_base_before_reentry() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let first = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(first);
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(first));
    // Navigation destroys the engine before releasing its borrowed source.
    drop(engine);
    shared.release_inactive_resources();
    assert!(shared.asset_bytes.is_none());
    assert!(shared.base_bits.is_none());
    assert!(shared.base_gray.is_none());
    assert!(!shared.base_ready());

    // A later activation owns a fresh pack and rebuilds its stationary base.
    shared.asset_bytes = seed_shared().asset_bytes;
    let mut engine = fresh_engine();
    let second = target_at(t(9, 2, 0), model::MinuteIntent::Fast);
    engine.set_target(second);
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(second));
    assert!(shared.base_ready());
}

#[test]
fn interrupted_base_navigation_restarts_safely() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    engine.set_target(target_at(t(9, 1, 0), model::MinuteIntent::Clean));
    for _ in 0..5 {
        assert_eq!(engine.service(&mut shared), None);
    }
    // Mid-pass: both caches allocated together but definitively not
    // ready (completion still pending).
    assert!(shared.base_bits.is_some());
    assert!(shared.base_gray.is_some());
    assert!(
        !shared.base_ready(),
        "partial base must never read as ready"
    );

    // Navigation drops the engine mid-pass, then releases both partial
    // buffers and the pack. Reentry starts with fresh bytes and base rows.
    drop(engine);
    shared.release_inactive_resources();
    assert!(shared.base_bits.is_none());
    assert!(shared.base_gray.is_none());
    shared.asset_bytes = seed_shared().asset_bytes;
    let mut engine = fresh_engine();
    assert!(
        !shared.base_ready(),
        "partial base stays unready across reentry"
    );
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    while !shared.base_ready() {
        assert_eq!(engine.service(&mut shared), None);
        assert!(engine.frame_ready().is_none());
    }
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(target));
}

#[test]
fn missing_gray_cache_never_ready_and_frame_never_leaks() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    // Simulate the old single-cache shape: bits allocated and marked
    // complete, gray never populated. Readiness must still be false and
    // no frame may leak; service must (re)start the base pass instead.
    shared.base_bits = Some(
        firmware::psram::acquire_runtime_bundle(&[phase_arena::PartRequest {
            bytes: PACKED_LEN,
            align: 4,
            shape: phase_arena::PartShape::Contiguous,
        }])
        .expect("host bits"),
    );
    shared.base_gray = None;
    shared.base_complete = true;
    assert!(
        !shared.base_ready(),
        "bits without gray must never read as ready"
    );
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    assert_eq!(engine.service(&mut shared), None);
    assert!(engine.frame_ready().is_none());
    // The restarted pass completes both caches together.
    tick_until(&mut engine, &mut shared, 5_000, |engine, shared| {
        assert!(engine.frame_ready().is_none());
        shared.base_ready()
    });
    assert_eq!(
        shared
            .base_gray
            .as_ref()
            .and_then(|gray| gray.part(0))
            .map(|part| part.len()),
        Some(engine::GRAY_CACHE_LEN)
    );
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(target));
}

#[test]
fn incomplete_caches_never_ready() {
    let mut shared = seed_shared();
    // Both caches allocated but the pass never finished: not ready.
    shared.base_bits = Some(
        firmware::psram::acquire_runtime_bundle(&[phase_arena::PartRequest {
            bytes: PACKED_LEN,
            align: 4,
            shape: phase_arena::PartShape::Contiguous,
        }])
        .expect("host bits"),
    );
    shared.base_gray = Some(
        firmware::psram::acquire_runtime_bundle(&[phase_arena::PartRequest {
            bytes: engine::GRAY_CACHE_LEN,
            align: 4,
            shape: phase_arena::PartShape::Contiguous,
        }])
        .expect("host gray"),
    );
    shared.base_complete = false;
    assert!(
        !shared.base_ready(),
        "allocated-but-incomplete caches must never read as ready"
    );
}

#[test]
fn successive_minutes_render_after_publish() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let first = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(first);
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(first));

    // Publishing only advances minute deduplication; the completed base
    // stays cached for the next minute.
    engine.mark_published(first, 1_500);
    assert!(shared.base_ready());
    let second = target_at(t(9, 2, 0), model::MinuteIntent::Fast);
    engine.set_target(second);
    tick_until(&mut engine, &mut shared, 5_000, |engine, _| {
        engine.frame_ready().is_some()
    });
    assert_eq!(engine.frame_ready(), Some(second));
    assert!(engine.failed().is_none());
}

#[test]
fn rtc_rounding_keeps_completed_future_frame_without_rerender() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let target = target_at(t(9, 10, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    tick_until(&mut engine, &mut shared, 300, |engine, _| {
        engine.frame_ready().is_some()
    });
    let bits = engine.staging_bits().unwrap().to_vec();
    let pointer = engine.staging_bits().unwrap().as_ptr();
    let preceding_minute = model::epoch_minute(t(9, 9, 59));
    engine.observe_estimated_minute(preceding_minute);
    assert_eq!(
        engine.settle_minute(target, preceding_minute),
        FrameSettlement::Hold
    );
    assert_eq!(engine.frame_ready(), Some(target));
    assert_eq!(engine.staging_bits().unwrap(), bits);
    assert_eq!(engine.staging_bits().unwrap().as_ptr(), pointer);
    assert_eq!(
        engine.settle_minute(target, target.epoch_minute),
        FrameSettlement::Current
    );
    engine.mark_published(target, 60_000);
    assert_eq!(
        engine.settle_minute(target, target.epoch_minute),
        FrameSettlement::Hold
    );
}

#[test]
fn completed_stale_frame_or_large_clock_backstep_retargets_clean() {
    for local_seconds in [t(9, 11, 0), t(8, 55, 0)] {
        let mut engine = fresh_engine();
        let mut shared = seed_shared();
        let target = target_at(t(9, 10, 0), model::MinuteIntent::Clean);
        engine.set_target(target);
        tick_until(&mut engine, &mut shared, 300, |engine, _| {
            engine.frame_ready().is_some()
        });
        let current = model::epoch_minute(local_seconds);
        assert_eq!(
            engine.settle_minute(target, current),
            FrameSettlement::Retargeted
        );
        assert_eq!(engine.frame_ready(), None);
        assert_eq!(
            engine.target(),
            Some(target_at(local_seconds, model::MinuteIntent::Clean))
        );
        assert!(shared.base_ready());
    }
}

#[test]
fn estimated_rollover_retargets_loading_work_without_restarting_base() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    engine.set_target(target_at(t(9, 9, 0), model::MinuteIntent::Clean));
    engine.service(&mut shared);
    assert_eq!(engine.phase_label(), "rendering_base");
    let current = model::epoch_minute(t(9, 10, 0));
    engine.observe_estimated_minute(current);
    assert_eq!(engine.target().unwrap().epoch_minute, current);
    assert_eq!(engine.phase_label(), "rendering_base");
    let remaining = tick_until(&mut engine, &mut shared, 100, |_, shared| {
        shared.base_ready()
    });
    assert_eq!(remaining, 600_u32.div_ceil(engine::BASE_ROWS_PER_TICK) - 1);
}

fn tick_until_frame(engine: &mut RenderEngine, shared: &mut SharedCache) -> TargetMinute {
    let mut ticks = 0;
    loop {
        if let Some(target) = engine.frame_ready() {
            return target;
        }
        assert!(ticks < 5_000, "engine stalled: no frame within 5000 ticks");
        engine.service(shared);
        ticks += 1;
    }
}

#[test]
fn profile_starts_zeroed_before_any_frame() {
    let engine = fresh_engine();
    let profile = engine.profile();
    assert!(!profile.started());
    assert_eq!(profile.rows(), 0);
    assert_eq!(profile.pixels(), 0);
    assert_eq!(profile.batches(), 0);
    assert_eq!(profile.wall_us(), 0);
    assert_eq!(profile.active_us(), 0);
    assert_eq!(profile.shade_us(), 0);
    assert_eq!(profile.dither_pack_us(), 0);
    assert_eq!(profile.restore_us(), 0);
    assert_eq!(profile.gap_us(), 0);
}

#[test]
fn full_frame_counts_every_row_pixel_and_batch() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    assert_eq!(tick_until_frame(&mut engine, &mut shared), target);

    let profile = engine.profile();
    assert!(profile.started());
    assert_eq!(profile.epoch_minute(), target.epoch_minute);
    assert_eq!(profile.rows(), engine::CLOCK_HEIGHT);
    assert_eq!(profile.pixels(), engine::CLOCK_HEIGHT * engine::CLOCK_WIDTH);
    assert_eq!(
        profile.batches(),
        engine::CLOCK_HEIGHT.div_ceil(engine::FRAME_ROWS_PER_TICK)
    );
    assert_eq!(
        profile.unchanged_pixels(),
        engine::CLOCK_HEIGHT * engine::CLOCK_WIDTH
    );
    assert_eq!(profile.hand_pixels(), 0);
    assert_eq!(profile.shadow_pixels(), 0);
    assert!(profile.wall_us() >= profile.active_us());
    assert_eq!(
        profile.gap_us(),
        profile.wall_us().saturating_sub(profile.active_us())
    );
}

#[test]
fn service_ticks_accumulate_without_resetting() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    engine.set_target(target_at(t(9, 1, 0), model::MinuteIntent::Clean));
    while !shared.base_ready() {
        engine.service(&mut shared);
    }
    for _ in 0..3 {
        engine.service(&mut shared);
    }
    let mid = engine.profile();
    assert!(mid.started(), "arming starts the interval");
    assert!(mid.rows() > 0 && mid.rows() < engine::CLOCK_HEIGHT);
    assert!(mid.batches() >= 3);
}

#[test]
fn fresh_arm_resets_profile_for_next_minute() {
    let mut engine = fresh_engine();
    let mut shared = seed_shared();
    let first = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(first);
    assert_eq!(tick_until_frame(&mut engine, &mut shared), first);

    engine.mark_published(first, 1_500);
    let second = target_at(t(9, 2, 0), model::MinuteIntent::Fast);
    engine.set_target(second);
    assert_eq!(tick_until_frame(&mut engine, &mut shared), second);

    let profile = engine.profile();
    assert_eq!(profile.epoch_minute(), second.epoch_minute);
    assert_eq!(profile.rows(), engine::CLOCK_HEIGHT);
    assert_eq!(profile.pixels(), engine::CLOCK_HEIGHT * engine::CLOCK_WIDTH);
}

/// Row-major MSB-first bit plane for the whole-frame oracle.
struct OraclePlane {
    width: u32,
    height: u32,
    bits: std::vec::Vec<u8>,
}

impl OraclePlane {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bits: std::vec![0u8; width as usize * height as usize / 8],
        }
    }
}

impl analog_clock::Surface for OraclePlane {
    fn width(&self) -> i32 {
        self.width as i32
    }

    fn height(&self) -> i32 {
        self.height as i32
    }

    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let bit = y as usize * self.width as usize + x as usize;
        let mask = 0x80 >> (bit % 8);
        if ink {
            self.bits[bit / 8] |= mask;
        } else {
            self.bits[bit / 8] &= !mask;
        }
    }
}

/// Streamed cached frame must equal the original full-ray shader: shade
/// every row with `render_dynamic_row`, dither the full plane with the
/// product frame profiles, then restore dial-only (`Footprint::NONE`)
/// pixels from the shared packed base — the same order the engine uses.
/// Angles and profiles match the product frame pass exactly, over the
/// non-transparent textured pack (transparent packs cannot exercise the
/// hand path).
#[test]
fn streamed_frame_matches_whole_frame_ray_shader() {
    use analog_clock::{
        angles_for_time, dither_regions, regional_scratch_len, render_dynamic_row, ClockScene,
        DitherAlgorithm, DitherRegion, Footprint, RegionDithers,
    };

    let mut engine = fresh_engine();
    let mut shared = seed_textured_shared();
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    assert_eq!(tick_until_frame(&mut engine, &mut shared), target);
    let staging = engine.staging_bits().expect("frame staging").to_vec();
    assert_eq!(staging.len(), PACKED_LEN);
    let base = shared
        .base_bits
        .as_ref()
        .expect("completed packed base")
        .part(0)
        .expect("completed packed base")
        .to_vec();
    assert_eq!(
        shared
            .base_gray
            .as_ref()
            .and_then(|gray| gray.part(0))
            .map(|part| part.len()),
        Some(engine::GRAY_CACHE_LEN)
    );

    let width = engine::CLOCK_WIDTH;
    let height = engine::CLOCK_HEIGHT;
    let pixels = width as usize * height as usize;
    let assets = shared
        .asset_bytes
        .as_ref()
        .expect("seeded bytes")
        .borrowed()
        .expect("textured pack decodes");
    let dial = assets.dial();
    assert!(dial.validate(), "textured dial validates");
    let hands = assets.hands();
    assert!(hands.validate(), "textured hands validate");
    let (hour_angle, minute_angle) = angles_for_time(target.hour, target.minute, 0);
    let mut scene = ClockScene::for_size(width, height);
    scene.dial = Some(dial);
    scene.hand_darkness = 0.85;
    scene.hour_angle = hour_angle;
    scene.minute_angle = minute_angle;

    let mut gray = std::vec![0u8; pixels];
    let mut regions = std::vec![DitherRegion::Background; pixels];
    let mut footprint = std::vec![Footprint::NONE; pixels];
    for y in 0..height {
        let row = y as usize * width as usize;
        render_dynamic_row(
            &scene,
            &hands,
            y,
            &mut gray[row..row + width as usize],
            &mut regions[row..row + width as usize],
            &mut footprint[row..row + width as usize],
        );
    }
    // The textured pack must actually cover hand pixels; otherwise this
    // test degrades to the transparent-pack case it replaces.
    let touched = footprint.iter().filter(|foot| !foot.is_empty()).count();
    assert!(
        touched > 1_000,
        "textured hands must cover pixels, got {touched}"
    );

    let profiles = RegionDithers {
        background: DitherAlgorithm::Atkinson,
        clock: DitherAlgorithm::Bayer8,
        hands: DitherAlgorithm::FloydSteinberg,
        shadows: DitherAlgorithm::FloydSteinberg,
    };
    let mut errors = std::vec![0.0f32; regional_scratch_len(width as usize).expect("scratch len")];
    let mut plane = OraclePlane::new(width, height);
    dither_regions(
        width,
        height,
        &gray,
        &regions,
        &profiles,
        &mut errors,
        &mut plane,
    )
    .expect("oracle dithers");
    // Replacement composition after quantization, mirroring the engine:
    // dial-only pixels keep the cached stationary base.
    for y in 0..height as usize {
        for x in 0..width as usize {
            if footprint[y * width as usize + x].is_empty() {
                let bit = y * width as usize + x;
                let mask = 0x80 >> (bit % 8);
                if packed_bit_at(&base, x, y, width as usize) {
                    plane.bits[bit / 8] |= mask;
                } else {
                    plane.bits[bit / 8] &= !mask;
                }
            }
        }
    }
    assert_eq!(
        plane.bits.len(),
        staging.len(),
        "oracle and staging share the packed geometry"
    );
    let first_mismatch = plane
        .bits
        .iter()
        .zip(staging.iter())
        .position(|(a, b)| a != b);
    assert!(
        first_mismatch.is_none(),
        "streamed frame differs from ray-shader oracle at byte {first_mismatch:?}"
    );
}

#[test]
fn active_upload_replacement_reloads_same_target() {
    use firmware::storage::clock_assets::{self, AssetReadCompletion};

    clock_assets::enable_requests(true);
    let mut engine = fresh_engine();
    let mut shared = SharedCache::new();
    engine.service(&mut shared);
    let first_id = clock_assets::last_request_id();
    assert_ne!(first_id, 0);
    let first_generation = clock_assets::upload_generation();
    clock_assets::complete(AssetReadCompletion {
        id: first_id,
        generation: first_generation,
        result: Ok(seed_shared().asset_bytes.take().expect("valid pack")),
    });
    let target = target_at(t(9, 1, 0), model::MinuteIntent::Clean);
    engine.set_target(target);
    assert_eq!(tick_until_frame(&mut engine, &mut shared), target);
    assert!(shared.base_ready());
    assert!(engine.staging_bits().is_some());

    clock_assets::note_committed_upload();
    let second_generation = clock_assets::upload_generation();
    assert_ne!(second_generation, first_generation);
    clock_assets::complete(AssetReadCompletion {
        id: first_id,
        generation: first_generation,
        result: Err(clock_assets::AssetReadError::Busy),
    });
    engine.service(&mut shared);
    assert!(shared.asset_bytes.is_none());
    assert!(!shared.base_ready());
    assert!(engine.staging_bits().is_none());
    assert_eq!(engine.target(), Some(target));
    assert_eq!(engine.phase_label(), "awaiting_minute");
    assert!(engine.failed().is_none());
    let second_id = clock_assets::last_request_id();
    assert_ne!(second_id, first_id);

    for _ in 0..3 {
        let pending = clock_assets::last_request_id();
        clock_assets::complete(AssetReadCompletion {
            id: pending,
            generation: second_generation,
            result: Err(clock_assets::AssetReadError::Busy),
        });
        engine.service(&mut shared);
        assert!(engine.failed().is_none());
        engine.service(&mut shared);
    }
    let third_id = clock_assets::last_request_id();
    clock_assets::complete(AssetReadCompletion {
        id: third_id,
        generation: second_generation,
        result: Ok(seed_shared().asset_bytes.take().expect("valid pack")),
    });
    assert_eq!(tick_until_frame(&mut engine, &mut shared), target);
    assert!(shared.base_ready());
    assert!(engine.failed().is_none());
    clock_assets::enable_requests(false);
}

#[test]
fn older_completion_keeps_newer_request_identity() {
    use firmware::storage::clock_assets::{self, AssetReadCompletion};

    clock_assets::enable_requests(true);
    let mut engine = fresh_engine();
    let mut shared = SharedCache::new();
    engine.service(&mut shared);
    let old_id = clock_assets::last_request_id();
    clock_assets::complete(AssetReadCompletion {
        id: old_id,
        generation: clock_assets::upload_generation(),
        result: Err(clock_assets::AssetReadError::Busy),
    });
    engine.service(&mut shared);
    engine.service(&mut shared);
    let current_id = clock_assets::last_request_id();
    assert_ne!(old_id, current_id);

    clock_assets::complete(AssetReadCompletion {
        id: old_id,
        generation: clock_assets::upload_generation(),
        result: Err(clock_assets::AssetReadError::NotFound),
    });
    engine.service(&mut shared);
    clock_assets::complete(AssetReadCompletion {
        id: current_id,
        generation: clock_assets::upload_generation(),
        result: Ok(seed_shared().asset_bytes.take().expect("valid pack")),
    });
    engine.service(&mut shared);
    assert!(shared.asset_bytes.is_some());
    assert!(engine.failed().is_none());
    clock_assets::enable_requests(false);
}
