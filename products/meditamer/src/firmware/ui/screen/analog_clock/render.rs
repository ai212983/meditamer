//! Incremental cooperative renderer for the runtime analog-clock ambient.
//!
//! The device retains one stationary grayscale dial in PSRAM, without
//! full dynamic gray/region/footprint planes. Per display
//! tick this advances a bounded number of rows: three caller-owned gray
//! rows, three region rows, and three footprint rows (the Atkinson
//! lookahead), one float error scratch, and two packed bit planes (the
//! cached stationary base and the current frame staging). The whole-frame
//! core passes are never duplicated here -- every pixel goes through the
//! core's own base-row, mask preparation, and cached material passes, plus the core's
//! `streaming::StreamingDither`, so streamed output agrees with the
//! whole-frame path by construction.
//!
//! Map views borrow the validated buffer only during each render step. The
//! buffer and stationary base are released after the clock screen closes.

use allocator_api2::vec::Vec;
use analog_clock::StreamingDither;
use analog_clock::{
    angles_for_time, packed_bits_len, render_base_row, ClockScene, DitherRegion, Footprint,
};

use crate::firmware::psram::{acquire_runtime_bundle, alloc_large_byte_buffer, BufferPlacement};
use crate::firmware::storage::clock_assets::{
    request_assets, take_assets, upload_generation, AssetBuffers, AssetReadError,
};
use phase_arena::{PartRequest, PartShape, RuntimeBundle};

use super::model::{self, AssetQueueAttempt, AssetRetryPolicy, MinuteIntent, MinuteTracker};

#[path = "frame_profile.rs"]
pub(crate) mod frame_profile;

#[path = "render/primitives.rs"]
mod primitives;
#[path = "shadow_workspace.rs"]
mod shadow_workspace;
use primitives::{base_profiles, decode_shared, frame_profiles, packed_bit, BitWriter};
use shadow_workspace::{RowOutputs, ShadowWorkspace};

use embassy_time::Instant;

use self::frame_profile::FrameProfile;

/// Native dial geometry, 1:1 with the source pack.
pub const CLOCK_WIDTH: u32 = model::WIDTH_PX;
pub const CLOCK_HEIGHT: u32 = model::HEIGHT_PX;
/// Packed-bit bytes for one 600x600 frame.
pub const PACKED_LEN: usize = 45_000;
/// Canvas bytes (L8, one byte per pixel) for the publish buffer.
pub const CANVAS_LEN: usize = 360_000;
/// Retained grayscale dial cache bytes: one byte per pixel, populated
/// during the original base pass (no extra shading pass).
pub const GRAY_CACHE_LEN: usize = CANVAS_LEN;
/// Caller-owned mask scratch bytes: three width-byte open-count planes
/// (dial, lower hand, upper hand) reused for every shaded row.
pub const MASK_SCRATCH_LEN: usize = 3 * (CLOCK_WIDTH as usize);
/// Cooperative budgets per display tick. CPU render cost on device is
/// measured per frame with phase profiling; these stay small: base rows are cheap (dial only),
/// compose rows carry the full lighting evaluation.
pub const BASE_ROWS_PER_TICK: u32 = 16;
pub const FRAME_ROWS_PER_TICK: u32 = 4;
// Asset retries per activation are bounded by the retry policy
// ([`AssetRetryPolicy::MAX_ATTEMPTS`]) before parking on the placeholder,
// so the renderer and its host tests share one budget.

/// One wall minute under render, with the refresh class the policy chose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetMinute {
    pub epoch_minute: u64,
    pub hour: u8,
    pub minute: u8,
    pub intent: MinuteIntent,
}

impl TargetMinute {
    pub fn new(epoch_minute: u64, local_epoch_seconds: u32, intent: MinuteIntent) -> Self {
        let (hour, minute) = model::clock_h_m(local_epoch_seconds);
        Self {
            epoch_minute,
            hour,
            minute,
            intent,
        }
    }
}

/// What is wrong with the clock surface. Never a fabricated wall time: on
/// any of these the screen keeps its placeholder and stays navigable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockFailure {
    AssetLoad,
    Workspace,
    Render,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameSettlement {
    Current,
    Hold,
    Retargeted,
}

/// Backend-owned resources shared with the active clock screen.
pub struct SharedCache {
    pub asset_bytes: Option<AssetBuffers>,
    pub canvas: Option<&'static render::lvgl_adapter::ExternalL8CanvasBuffer>,
    pub draw_unit: Option<&'static render::lvgl_adapter::PreparedL8DrawUnit>,
    pub base_bits: Option<RuntimeBundle<esp_alloc::ExternalMemory>>,
    pub base_gray: Option<RuntimeBundle<esp_alloc::ExternalMemory>>,
    /// True only once the cached base holds every completed row. Allocation
    /// alone is not readiness: the cooperative pass fills the buffer over
    /// many ticks, and a navigation can drop the engine mid-pass leaving a
    /// partial buffer behind.
    pub base_complete: bool,
}

impl SharedCache {
    pub const fn new() -> Self {
        Self {
            asset_bytes: None,
            canvas: None,
            draw_unit: None,
            base_bits: None,
            base_gray: None,
            base_complete: false,
        }
    }

    /// Whether the shared base cache holds a fully rendered stationary
    /// base. A merely allocated (partial) buffer is never ready, including
    /// after a navigation dropped the engine mid-pass.
    pub fn base_ready(&self) -> bool {
        self.base_complete
            && self
                .base_bits
                .as_ref()
                .is_some_and(|b| b.part(0).is_some_and(|s| s.len() == PACKED_LEN))
            && self
                .base_gray
                .as_ref()
                .is_some_and(|b| b.part(0).is_some_and(|s| s.len() == GRAY_CACHE_LEN))
    }

    /// Reserve the stationary base while PSRAM still has a large contiguous
    /// span. The split map stream can then fill the remaining smaller holes.
    fn reserve_base_storage(&mut self) -> Result<(), ClockFailure> {
        if self
            .base_gray
            .as_ref()
            .is_some_and(|b| b.part(0).is_some_and(|s| s.len() == GRAY_CACHE_LEN))
            && self
                .base_bits
                .as_ref()
                .is_some_and(|b| b.part(0).is_some_and(|s| s.len() == PACKED_LEN))
        {
            return Ok(());
        }
        self.base_gray = None;
        self.base_bits = None;
        self.base_complete = false;
        let gray_len = (CLOCK_WIDTH as usize)
            .checked_mul(CLOCK_HEIGHT as usize)
            .ok_or(ClockFailure::Workspace)?;
        let bits_len = packed_bits_len(CLOCK_WIDTH, CLOCK_HEIGHT).ok_or(ClockFailure::Workspace)?;
        debug_assert_eq!(gray_len, GRAY_CACHE_LEN);
        debug_assert_eq!(bits_len, PACKED_LEN);
        let gray = acquire_clock_workspace(gray_len)?;
        let bits = acquire_clock_workspace(bits_len)?;
        self.base_gray = Some(gray);
        self.base_bits = Some(bits);
        console::println!(
            "CLOCK_BASE status=reserved bytes={}",
            GRAY_CACHE_LEN + PACKED_LEN
        );
        Ok(())
    }

    /// Lazily allocate and leak the 360 KB L8 publish canvas once per boot.
    /// Strict PSRAM: an internal-memory fallback is rejected, not kept, and
    /// the 4-byte adapter alignment is verified on the owned buffer *before*
    /// it is leaked, so a rejection frees the buffer instead of pinning it.
    /// Callers allocate the small keeper wrapper *before* calling this, so a
    /// wrapper OOM never costs the large buffer and retries cleanly.
    pub fn canvas_or_allocate(&mut self) -> Result<&'static mut [u8], ClockFailure> {
        if self.canvas.is_some() {
            return Err(ClockFailure::Workspace);
        }
        let buffer = alloc_large_byte_buffer(CANVAS_LEN).map_err(|_| ClockFailure::Workspace)?;
        if buffer.placement() != BufferPlacement::Psram {
            return Err(ClockFailure::Workspace);
        }
        if !(buffer.as_slice().as_ptr() as usize).is_multiple_of(4) {
            return Err(ClockFailure::Workspace);
        }
        Ok(buffer.into_static_mut_slice())
    }

    /// Adopt one successfully loaded, validated pack buffer.
    pub fn adopt_asset_buffer(&mut self, buffer: AssetBuffers) {
        if self.asset_bytes.is_none() {
            self.asset_bytes = Some(buffer);
        }
    }

    /// Release the large clock-only resources after the screen is destroyed.
    pub fn release_inactive_resources(&mut self) {
        if let Some(bytes) = self.asset_bytes.take() {
            #[cfg(target_os = "none")]
            let released = bytes.total_bytes();
            #[cfg(not(target_os = "none"))]
            let released = bytes
                .maps
                .iter()
                .map(|map| map.as_slice().len())
                .sum::<usize>();
            console::println!("CLOCK_ASSETS status=released bytes={}", released);
            drop(bytes);
        }
        self.base_bits = None;
        self.base_gray = None;
        self.base_complete = false;
        // A read accepted just before navigation can finish after exit.
        // Reap it here so the single-outstanding guard does not remain held.
        drop(take_assets());
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Placeholder,
    RenderingBase { next_y: u32 },
    AwaitingMinute,
    RenderingFrame,
    FrameReady,
    Failed(ClockFailure),
}

/// Per-activation render workspace. All large buffers are PSRAM-backed and
/// released when the screen exits; only small pointers live on the stack.
pub struct RenderEngine {
    maps_ready: bool,
    asset_request_id: Option<u32>,
    asset_generation: u32,
    phase: Phase,
    tracker: MinuteTracker,
    target: Option<TargetMinute>,
    frame_angles: Option<(f32, f32)>,
    frame_armed: bool,
    base_stream: StreamingDither,
    frame_stream: StreamingDither,
    gray: [Vec<u8, esp_alloc::ExternalMemory>; 3],
    regions: [Vec<DitherRegion, esp_alloc::ExternalMemory>; 3],
    footprint: [Vec<Footprint, esp_alloc::ExternalMemory>; 3],
    errors: Vec<f32, esp_alloc::ExternalMemory>,
    staging: Option<RuntimeBundle<esp_alloc::ExternalMemory>>,
    ring_y: u32,
    ring_armed: bool,
    asset_policy: AssetRetryPolicy,
    profile: FrameProfile,
    shadows: Vec<ShadowWorkspace, esp_alloc::ExternalMemory>,
}

fn acquire_clock_workspace(
    byte_len: usize,
) -> Result<RuntimeBundle<esp_alloc::ExternalMemory>, ClockFailure> {
    acquire_runtime_bundle(&[PartRequest {
        bytes: byte_len,
        align: 4,
        shape: PartShape::Contiguous,
    }])
    .map_err(|_| ClockFailure::Workspace)
}

fn psram_vec<T: Clone>(
    len: usize,
    value: T,
) -> Result<Vec<T, esp_alloc::ExternalMemory>, ClockFailure> {
    let mut vec = Vec::new_in(esp_alloc::ExternalMemory);
    vec.try_reserve_exact(len)
        .map_err(|_| ClockFailure::Workspace)?;
    vec.resize(len, value);
    Ok(vec)
}

impl RenderEngine {
    pub fn new() -> Result<Self, ClockFailure> {
        debug_assert_eq!(packed_bits_len(CLOCK_WIDTH, CLOCK_HEIGHT), Some(PACKED_LEN));
        let width = CLOCK_WIDTH as usize;
        let errors_len =
            analog_clock::regional_scratch_len(width).ok_or(ClockFailure::Workspace)?;
        Ok(Self {
            maps_ready: false,
            asset_request_id: None,
            asset_generation: upload_generation(),
            phase: Phase::Placeholder,
            tracker: MinuteTracker::new(),
            target: None,
            frame_angles: None,
            frame_armed: false,
            base_stream: StreamingDither::new(CLOCK_WIDTH, CLOCK_HEIGHT, base_profiles())
                .map_err(|_| ClockFailure::Workspace)?,
            frame_stream: StreamingDither::new(CLOCK_WIDTH, CLOCK_HEIGHT, frame_profiles())
                .map_err(|_| ClockFailure::Workspace)?,
            gray: [
                psram_vec(width, 0u8)?,
                psram_vec(width, 0u8)?,
                psram_vec(width, 0u8)?,
            ],
            regions: [
                psram_vec(width, DitherRegion::Background)?,
                psram_vec(width, DitherRegion::Background)?,
                psram_vec(width, DitherRegion::Background)?,
            ],
            footprint: [
                psram_vec(width, Footprint::NONE)?,
                psram_vec(width, Footprint::NONE)?,
                psram_vec(width, Footprint::NONE)?,
            ],
            errors: psram_vec(errors_len, 0.0f32)?,
            staging: None,
            ring_y: 0,
            ring_armed: false,
            asset_policy: AssetRetryPolicy::new(),
            profile: FrameProfile::new(),
            shadows: shadow_workspace::allocate()?,
        })
    }

    /// Park the screen when the retry policy exhausts its budget.
    fn note_asset_failure(&mut self, parked: bool) {
        if parked {
            self.phase = Phase::Failed(ClockFailure::AssetLoad);
        }
    }

    pub fn tracker(&self) -> &MinuteTracker {
        &self.tracker
    }

    pub fn tracker_mut(&mut self) -> &mut MinuteTracker {
        &mut self.tracker
    }

    pub fn target(&self) -> Option<TargetMinute> {
        self.target
    }

    pub fn frame_ready(&self) -> Option<TargetMinute> {
        (self.phase == Phase::FrameReady)
            .then_some(self.target)
            .flatten()
    }

    /// Inline per-frame phase accounting for the in-flight compose frame.
    #[cfg(test)]
    pub fn profile(&self) -> &FrameProfile {
        &self.profile
    }

    /// Follow minute rollover using the monotonic estimate, including
    /// while SD/base work is pending. Future prefetches keep their work.
    pub fn observe_estimated_minute(&mut self, current: u64) {
        if self
            .target
            .is_some_and(|target| target.epoch_minute < current)
        {
            self.target_current_clean(current);
        }
    }

    /// A just-prefetched future frame survives RTC rounding at a boundary.
    /// Stale frames and larger backwards jumps need a new current frame.
    pub fn settle_minute(&mut self, completed: TargetMinute, current: u64) -> FrameSettlement {
        if self.frame_ready() != Some(completed) {
            return FrameSettlement::Hold;
        }
        if current == completed.epoch_minute {
            return FrameSettlement::Current;
        }
        if current < completed.epoch_minute && completed.epoch_minute - current == 1 {
            return FrameSettlement::Hold;
        }
        self.target_current_clean(current);
        FrameSettlement::Retargeted
    }

    fn target_current_clean(&mut self, current: u64) {
        self.set_target(TargetMinute::new(
            current,
            model::epoch_to_local_seconds(current),
            MinuteIntent::Clean,
        ));
    }

    pub fn failed(&self) -> Option<ClockFailure> {
        match self.phase {
            Phase::Failed(failure) => Some(failure),
            _ => None,
        }
    }

    /// Whether frame rows are actively stepping: armed and rendering, not
    /// yet ready. Matches the private [`Phase`] directly, never a label
    /// comparison. Drives the render-budget sampler: base/asset/placeholder
    /// work must not count toward the frame budget.
    pub fn is_rendering_frame(&self) -> bool {
        matches!(self.phase, Phase::RenderingFrame)
    }

    /// One-word engine phase for the activation probe. Returns a label
    /// rather than the private `Phase` so no visibility boundary moves.
    pub fn phase_label(&self) -> &'static str {
        match self.phase {
            Phase::Placeholder => "placeholder",
            Phase::RenderingBase { .. } => "rendering_base",
            Phase::AwaitingMinute => "awaiting_minute",
            Phase::RenderingFrame => "rendering_frame",
            Phase::FrameReady => "frame_ready",
            Phase::Failed(_) => "failed",
        }
    }

    pub fn staging_bits(&self) -> Option<&[u8]> {
        self.staging.as_ref()?.part(0)
    }

    /// Retarget to a freshly observed wall minute. A duplicate of the
    /// in-flight target only records it; arming (stream reset, scene angles,
    /// cleared staging) happens in [`service`] once maps, base cache, and
    /// staging are all available, so a target observed before the assets
    /// arrive still starts its frame.
    ///
    /// [`service`]: RenderEngine::service
    pub fn set_target(&mut self, target: TargetMinute) {
        if self
            .target
            .is_some_and(|current| current.epoch_minute == target.epoch_minute)
        {
            return;
        }
        self.target = Some(target);
        self.frame_armed = false;
        self.ring_armed = false;
        // A fresh minute never cancels the minute-independent stationary
        // base pass: resetting to `AwaitingMinute` here would orphan the
        // `RenderingBase { next_y }` cursor while the partial buffer stays
        // allocated, stalling the base forever.
        if !matches!(self.phase, Phase::RenderingBase { .. }) {
            self.phase = Phase::AwaitingMinute;
        }
    }

    /// Arm the recorded target for row stepping. Idempotent: a no-op once
    /// the current target is armed.
    fn arm_frame(&mut self, shared: &SharedCache) {
        let Some(target) = self.target else {
            return;
        };
        if self.frame_armed || !self.maps_ready {
            return;
        }
        self.frame_stream.reset();
        if self.frame_stream.begin_frame(&mut self.errors).is_err() {
            self.phase = Phase::Failed(ClockFailure::Render);
            return;
        }
        let Some(maps) = shared.asset_bytes.as_ref().and_then(decode_shared) else {
            self.phase = Phase::Failed(ClockFailure::AssetLoad);
            return;
        };
        let mut scene = maps.scene;
        let (hour_angle, minute_angle) = angles_for_time(target.hour, target.minute, 0);
        scene.hour_angle = hour_angle;
        scene.minute_angle = minute_angle;
        self.frame_angles = Some((hour_angle, minute_angle));
        let Some(shadows) = self.shadows.first_mut() else {
            self.phase = Phase::Failed(ClockFailure::Workspace);
            return;
        };
        shadows.arm(&scene, &maps.hands);
        if self.staging.is_none() {
            let Some(bits_len) = packed_bits_len(CLOCK_WIDTH, CLOCK_HEIGHT) else {
                self.phase = Phase::Failed(ClockFailure::Workspace);
                return;
            };
            debug_assert_eq!(bits_len, PACKED_LEN);
            match acquire_clock_workspace(bits_len) {
                Ok(bundle) => self.staging = Some(bundle),
                Err(_) => {
                    self.phase = Phase::Failed(ClockFailure::Workspace);
                    return;
                }
            }
        }
        let Some(staging) = self.staging.as_mut() else {
            self.phase = Phase::Failed(ClockFailure::Workspace);
            return;
        };
        let Some(staging_slice) = staging.part_mut(0) else {
            self.phase = Phase::Failed(ClockFailure::Workspace);
            return;
        };
        staging_slice.fill(0);
        self.ring_armed = false;
        self.profile.reset(target.epoch_minute);
        self.frame_armed = true;
        self.phase = Phase::RenderingFrame;
    }

    fn maps_scene<'a>(&self, shared: &'a SharedCache) -> Option<ClockScene<'a>> {
        let maps = decode_shared(shared.asset_bytes.as_ref()?)?;
        let mut scene = maps.scene;
        if let Some((hour_angle, minute_angle)) = self.frame_angles {
            scene.hour_angle = hour_angle;
            scene.minute_angle = minute_angle;
        }
        Some(scene)
    }

    /// Advance asset loading, the stationary base pass, and the current
    /// frame by their per-tick row budgets. Returns a freshly completed
    /// frame target exactly once per frame.
    pub fn service(&mut self, shared: &mut SharedCache) -> Option<TargetMinute> {
        if !self.maps_ready && shared.asset_bytes.is_none() {
            if let Err(failure) = shared.reserve_base_storage() {
                self.phase = Phase::Failed(failure);
                return None;
            }
        }
        self.poll_assets(shared);
        if matches!(self.phase, Phase::Failed(_)) {
            shared.base_bits = None;
            shared.base_gray = None;
            shared.base_complete = false;
            return None;
        }
        if !self.maps_ready {
            return None;
        }
        // Readiness is completion, not allocation: the buffer is allocated
        // on the first base tick but holds every row only after the whole
        // cooperative pass finishes.
        if !shared.base_ready() {
            self.service_base_rows(shared);
            return None;
        }
        self.arm_frame(shared);
        self.service_frame_rows(shared)
    }

    /// Non-blocking asset poll: queue once, reap later ticks. Never awaits
    /// the SD read; the display task keeps owning I2C/SD power between
    /// these calls.
    fn poll_assets(&mut self, shared: &mut SharedCache) {
        let generation = upload_generation();
        if generation != self.asset_generation {
            self.asset_generation = generation;
            drop(shared.asset_bytes.take());
            shared.base_bits = None;
            shared.base_gray = None;
            shared.base_complete = false;
            self.staging = None;
            self.maps_ready = false;
            self.asset_request_id = None;
            self.frame_armed = false;
            self.ring_armed = false;
            self.asset_policy = AssetRetryPolicy::new();
            self.phase = if self.target.is_some() {
                Phase::AwaitingMinute
            } else {
                Phase::Placeholder
            };
            drop(take_assets());
            console::println!("CLOCK_ASSETS status=generation_invalidated");
        }
        if self.maps_ready || shared.asset_bytes.is_some() {
            // The bytes are already cached, so any queued completion is a
            // late one (e.g. it landed after an exit destroyed the previous
            // screen): free its buffer and release the single-outstanding
            // guard instead of wedging the loader on Busy. The synchronous
            // destroy drain alone cannot cover completions that arrive
            // after the exit.
            while take_assets().is_some() {}
        }
        if self.maps_ready || matches!(self.phase, Phase::Failed(_)) {
            return;
        }
        if let Some(bytes) = shared.asset_bytes.as_ref() {
            match decode_shared(bytes) {
                Some(_) => {
                    self.maps_ready = true;
                    self.phase = Phase::AwaitingMinute;
                    console::println!(
                        "CLOCK_ASSETS status=borrowed target_none={}",
                        self.target.is_none()
                    );
                }
                None => {
                    console::println!("CLOCK_ASSETS status=invalid");
                    self.phase = Phase::Failed(ClockFailure::AssetLoad);
                }
            }
            return;
        }
        match take_assets() {
            Some(completion)
                if Some(completion.id) != self.asset_request_id
                    || completion.generation != self.asset_generation =>
            {
                console::println!("CLOCK_ASSETS status=stale id={}", completion.id);
                if Some(completion.id) == self.asset_request_id {
                    self.asset_request_id = None;
                }
            }
            Some(completion) => {
                self.asset_request_id = None;
                match completion.result {
                    Ok(buffer) => {
                        console::println!("CLOCK_ASSETS status=complete id={}", completion.id);
                        shared.adopt_asset_buffer(buffer);
                        self.asset_policy.note_assets_ready();
                    }
                    Err(AssetReadError::Busy) => {
                        console::println!("CLOCK_ASSETS status=busy id={}", completion.id);
                    }
                    Err(_) => {
                        let parked = self.asset_policy.note_completion_error();
                        console::println!(
                            "CLOCK_ASSETS status=read_error id={} attempt={}",
                            completion.id,
                            self.asset_policy.failures()
                        );
                        self.note_asset_failure(parked);
                    }
                }
            }
            // No completion yet: queue once while idle, then keep polling.
            // `Busy` is the expected single-outstanding state while our
            // accepted request is in flight -- it consumes no retry budget
            // and never parks. Only a genuinely refused queue attempt
            // counts, and there is no retry loop or blocking wait here: the
            // display task keeps servicing SD power between these polls.
            None => match request_assets() {
                Ok(id) => {
                    self.asset_request_id = Some(id);
                    console::println!("CLOCK_ASSETS status=requested id={}", id);
                    self.asset_policy
                        .note_queue_attempt(AssetQueueAttempt::Accepted);
                }
                Err(AssetReadError::Busy) => {
                    self.asset_policy
                        .note_queue_attempt(AssetQueueAttempt::Busy);
                }
                Err(_) => {
                    console::println!("CLOCK_ASSETS status=request_error");
                    let parked = self
                        .asset_policy
                        .note_queue_attempt(AssetQueueAttempt::Refused);
                    self.note_asset_failure(parked);
                }
            },
        }
    }

    fn service_base_rows(&mut self, shared: &mut SharedCache) {
        if !matches!(
            self.phase,
            Phase::Placeholder | Phase::AwaitingMinute | Phase::RenderingBase { .. }
        ) {
            return;
        }
        if !matches!(self.phase, Phase::RenderingBase { .. }) {
            // (Re)start the pass over the reserved buffers. A partial base
            // is never marked complete, and exit releases both buffers.
            if shared.reserve_base_storage().is_err() {
                self.phase = Phase::Failed(ClockFailure::Workspace);
                return;
            }
            console::println!("CLOCK_BASE status=started");
            if self.base_stream.begin_frame(&mut self.errors).is_err() {
                self.phase = Phase::Failed(ClockFailure::Render);
                return;
            }
            self.phase = Phase::RenderingBase { next_y: 0 };
        }
        let Phase::RenderingBase { mut next_y } = self.phase else {
            return;
        };
        let Some(scene) = shared
            .asset_bytes
            .as_ref()
            .and_then(decode_shared)
            .map(|maps| maps.scene)
        else {
            self.phase = Phase::Failed(ClockFailure::AssetLoad);
            return;
        };
        let Some(base_gray) = shared.base_gray.as_mut().and_then(|b| b.part_mut(0)) else {
            return;
        };
        let Some(base_bits) = shared.base_bits.as_mut().and_then(|b| b.part_mut(0)) else {
            return;
        };
        let mut surface = BitWriter { bits: base_bits };
        for _ in 0..BASE_ROWS_PER_TICK {
            if next_y >= CLOCK_HEIGHT {
                break;
            }
            render_base_row(&scene, next_y, &mut self.gray[0], &mut self.regions[0]);
            // The validated imported dial suppresses all procedural furniture.
            // Refuse unsupported metadata rather than silently caching it wrong.
            if self.regions[0]
                .iter()
                .any(|r| *r != DitherRegion::Background)
            {
                self.phase = Phase::Failed(ClockFailure::Render);
                return;
            }
            let row = next_y as usize * CLOCK_WIDTH as usize;
            base_gray[row..row + CLOCK_WIDTH as usize].copy_from_slice(&self.gray[0]);
            let empty: &[DitherRegion] = &[];
            if self
                .base_stream
                .process_row(
                    next_y,
                    &self.gray[0],
                    &self.regions[0],
                    empty,
                    empty,
                    &mut self.errors,
                    &mut surface,
                )
                .is_err()
            {
                self.phase = Phase::Failed(ClockFailure::Render);
                return;
            }
            next_y += 1;
        }
        if next_y >= CLOCK_HEIGHT {
            if self.base_stream.finish().is_err() {
                self.phase = Phase::Failed(ClockFailure::Render);
                return;
            }
            console::println!("CLOCK_BASE status=complete");
            shared.base_complete = true;
            self.phase = Phase::AwaitingMinute;
        } else {
            self.phase = Phase::RenderingBase { next_y };
        }
    }

    fn service_frame_rows(&mut self, shared: &mut SharedCache) -> Option<TargetMinute> {
        if self.phase != Phase::RenderingFrame {
            return None;
        }
        let target = self.target?;
        let maps_scene = self.maps_scene(shared)?;
        if self.staging.is_none() || !shared.base_ready() {
            return None;
        }
        let tick_start_us = Instant::now().as_micros();
        let mut completed = false;
        for _ in 0..FRAME_ROWS_PER_TICK {
            let y = self.frame_stream.next_y();
            if y >= CLOCK_HEIGHT {
                break;
            }
            self.ensure_ring(maps_scene, y, shared);
            if !self.ring_armed {
                self.phase = Phase::Failed(ClockFailure::Render);
                self.profile
                    .note_batch(Instant::now().as_micros().saturating_sub(tick_start_us));
                return None;
            }
            // Split field borrows: the streaming pass needs the row window,
            // the error scratch, and the staging surface together.
            let Self {
                frame_stream,
                gray,
                regions,
                errors,
                staging,
                footprint,
                ..
            } = self;
            let staging = staging.as_mut()?;
            let (next, next2): (&[DitherRegion], &[DitherRegion]) = (
                if y + 1 < CLOCK_HEIGHT {
                    &regions[1]
                } else {
                    &[]
                },
                if y + 2 < CLOCK_HEIGHT {
                    &regions[2]
                } else {
                    &[]
                },
            );
            {
                let mut surface = BitWriter {
                    bits: staging.part_mut(0)?,
                };
                let dither_start_us = Instant::now().as_micros();
                let row_ok = frame_stream
                    .process_row(y, &gray[0], &regions[0], next, next2, errors, &mut surface)
                    .is_ok();
                self.profile
                    .add_dither_pack(Instant::now().as_micros().saturating_sub(dither_start_us));
                if !row_ok {
                    self.phase = Phase::Failed(ClockFailure::Render);
                    self.profile
                        .note_batch(Instant::now().as_micros().saturating_sub(tick_start_us));
                    return None;
                }
            }
            // Replacement composition after quantization: pixels the hands
            // and shadows never touch keep the fixed cached dial; hand and
            // shadow pixels keep the dynamic candidate. The selection never
            // feeds diffusion decisions.
            {
                let restore_start_us = Instant::now().as_micros();
                let (mut unchanged, mut hand, mut shadow) = (0u32, 0u32, 0u32);
                let base_bits = shared.base_bits.as_ref()?;
                let base = base_bits.part(0)?;
                let bits = staging.part_mut(0)?;
                for x in 0..CLOCK_WIDTH {
                    let pixel_footprint = footprint[0][x as usize];
                    if pixel_footprint.is_empty() {
                        unchanged += 1;
                        let bit = (y as usize) * (CLOCK_WIDTH as usize) + (x as usize);
                        let mask = 0x80 >> (bit % 8);
                        if packed_bit(base, x, y) {
                            bits[bit / 8] |= mask;
                        } else {
                            bits[bit / 8] &= !mask;
                        }
                    } else if pixel_footprint.hand() {
                        hand += 1;
                    } else if pixel_footprint.shadow() {
                        shadow += 1;
                    }
                }
                self.profile.note_footprint(unchanged, hand, shadow);
                self.profile
                    .add_restore(Instant::now().as_micros().saturating_sub(restore_start_us));
            }
            if !self.advance_ring(y, shared) {
                self.profile
                    .note_batch(Instant::now().as_micros().saturating_sub(tick_start_us));
                return None;
            }
            self.profile.note_row(CLOCK_WIDTH);
            if self.frame_stream.next_y() >= CLOCK_HEIGHT {
                completed = self.frame_stream.finish().is_ok();
                break;
            }
        }
        self.profile
            .note_batch(Instant::now().as_micros().saturating_sub(tick_start_us));
        if completed {
            self.phase = Phase::FrameReady;
            self.profile.finish();
            let profile = &self.profile;
            console::println!(
                "CLOCK_PROFILE epoch={} rows={} pixels={} batches={} wall_us={} active_us={} shade_us={} dither_pack_us={} restore_us={} gap_us={} unchanged={} hand={} shadow={}",
                profile.epoch_minute(),
                profile.rows(),
                profile.pixels(),
                profile.batches(),
                profile.wall_us(),
                profile.active_us(),
                profile.shade_us(),
                profile.dither_pack_us(),
                profile.restore_us(),
                profile.gap_us(),
                profile.unchanged_pixels(),
                profile.hand_pixels(),
                profile.shadow_pixels(),
            );
            if let Some(shadows) = self.shadows.first() {
                shadows.report(target.epoch_minute);
            }
            console::println!("CLOCK_FRAME status=complete");
            Some(target)
        } else {
            None
        }
    }

    /// Fill ring slots with dynamic rows `y`, `y+1`, `y+2` (clamped to the
    /// frame; past-bottom rows stay untouched and callers pass empty
    /// labels). Only full gray/regions/footprint enter the ring; the
    /// stationary base pass still shades through `render_base_row`; frame rows
    /// reuse its lit cache through the shared visibility-mask compositor.
    fn ensure_ring(&mut self, scene: ClockScene<'_>, y: u32, shared: &SharedCache) {
        if self.ring_armed && self.ring_y == y {
            return;
        }
        for (slot, row) in [y, y + 1, y + 2].iter().enumerate() {
            if *row < CLOCK_HEIGHT && !self.shade_row(scene, *row, slot, shared) {
                self.ring_armed = false;
                return;
            }
        }
        self.ring_y = y;
        self.ring_armed = true;
    }

    fn shade_row(
        &mut self,
        scene: ClockScene<'_>,
        y: u32,
        slot: usize,
        shared: &SharedCache,
    ) -> bool {
        let (Some(maps), Some(shadows)) = (
            shared.asset_bytes.as_ref().and_then(decode_shared),
            self.shadows.first_mut(),
        ) else {
            self.phase = Phase::Failed(ClockFailure::Workspace);
            return false;
        };
        let Some(base) = shared.base_gray.as_ref().and_then(|b| b.part(0)) else {
            self.phase = Phase::Failed(ClockFailure::Workspace);
            return false;
        };
        let start = y as usize * CLOCK_WIDTH as usize;
        let Some(base_row) = base.get(start..start + CLOCK_WIDTH as usize) else {
            self.phase = Phase::Failed(ClockFailure::Render);
            return false;
        };
        let shade_start_us = Instant::now().as_micros();
        let ok = shadows.render_row(
            &scene,
            &maps.hands,
            y,
            base_row,
            RowOutputs {
                gray: &mut self.gray[slot],
                regions: &mut self.regions[slot],
                footprint: &mut self.footprint[slot],
            },
        );
        self.profile
            .add_shade(Instant::now().as_micros().saturating_sub(shade_start_us));
        if !ok {
            self.phase = Phase::Failed(ClockFailure::Render);
        }
        ok
    }

    /// Rotate the lookahead window, shading each new row exactly once.
    fn advance_ring(&mut self, y: u32, shared: &SharedCache) -> bool {
        self.gray.swap(0, 1);
        self.gray.swap(1, 2);
        self.regions.swap(0, 1);
        self.regions.swap(1, 2);
        self.footprint.swap(0, 1);
        self.footprint.swap(1, 2);
        let next = y + 3;
        if next < CLOCK_HEIGHT {
            let Some(scene) = self.maps_scene(shared) else {
                return false;
            };
            if !self.shade_row(scene, next, 2, shared) {
                return false;
            }
        }
        self.ring_y = y + 1;
        true
    }

    /// Record a completed panel publish. Only this advances minute
    /// deduplication; rendering progress never does.
    pub fn mark_published(&mut self, target: TargetMinute, now_ms: u64) {
        if self
            .target
            .is_some_and(|current| current.epoch_minute == target.epoch_minute)
        {
            self.tracker.published(target.epoch_minute, now_ms);
            self.target = None;
            self.frame_armed = false;
            self.phase = Phase::AwaitingMinute;
        }
    }

    /// Discard a stale completed frame and retarget to the current minute.
    pub fn discard_stale(&mut self, target: TargetMinute) {
        if self
            .target
            .is_some_and(|current| current.epoch_minute == target.epoch_minute)
        {
            self.target = None;
            self.frame_armed = false;
            self.phase = Phase::AwaitingMinute;
        }
    }
}
