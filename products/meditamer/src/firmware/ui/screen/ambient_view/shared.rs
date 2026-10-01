//! Backend-owned Ambient Home resources. The publish canvas survives
//! navigation; the sky pack and mountain overlay are released after screen
//! exit.
//!
//! The per-activation staging buffer lives in the screen and is released on
//! exit.

#![forbid(unsafe_code)]

use render::lvgl_adapter::ExternalL8CanvasBuffer;

use crate::firmware::psram::{
    alloc_large_byte_buffer, BufferPlacement, ExternalValue, LargeByteBuffer,
};
#[cfg(feature = "asset-upload-http")]
use crate::firmware::service_mode;
use crate::firmware::storage::ambient_assets::{
    request_assets, take_assets, upload_generation, AssetBuffer, AssetReadError,
};
use crate::firmware::storage::mountain_assets;

/// L8 canvas pixels published per frame (600x600, one byte per pixel).
pub const CANVAS_PIXELS: usize = 360_000;
/// Packed staging frame bytes (600x600 bits, MSB-first).
pub const STAGING_LEN: usize = 45_000;
const _: () = {
    assert!(crate::firmware::ui::asset_bundles::AMBIENT_CORE_PARTS[0].bytes == STAGING_LEN);
    assert!(
        crate::firmware::ui::asset_bundles::AMBIENT_WITH_MOUNTAIN_PARTS[1].bytes
            == mountain_assets::OVERLAY_LEN
    );
};
/// Loading fill painted at entry: paper white, so the wait for the pack
/// reads as an empty framed field rather than unflushed noise.
/// Pure ink/paper only: the binary panel renders mid-grays (0x40-0xE4)
/// as white (proven on-panel), so gray fills and borders are invisible.
/// Distinct from the analog clock's gray placeholders, which share that
/// latent invisibility.
pub const PLACEHOLDER_LOADING: u8 = 0xFF;
/// Failure field, border, and cross painted once when the pack/PSRAM path
/// parks. The diagonal cross distinguishes "failed" from the loading
/// frame; all values are pure ink/paper for the reason above.
pub const PLACEHOLDER_FAILED_FIELD: u8 = 0xFF;
pub const PLACEHOLDER_FAILED_BORDER: u8 = 0x00;
pub const PLACEHOLDER_BORDER_PX: usize = 8;
/// Bounded loader retries per boot before parking on the placeholder.
/// Mirrors the analog clock's bounded-retry shape; failures keep their
/// retryable taxonomy (`AssetReadError::is_retryable`) but never spin the
/// display loop.
pub const MAX_ATTEMPTS: u8 = 5;
/// Delay between loader attempts; matches the clock's unavailable-retry
/// cadence so SD power cycles stay coherent across the two packs.
pub const RETRY_INTERVAL_MS: u64 = 30_000;
/// Poll cadence while a read is in flight. The completion has no wakeup of
/// its own, so the poll loop reaps it; channel ops only, no LVGL, and the
/// instant collapses back to the failure interval once the flight ends.
pub const COMPLETION_POLL_MS: u64 = 2_000;

/// Canvas allocation failure. Pack-load failures keep the loader's own
/// [`AssetReadError`](crate::firmware::storage::ambient_assets::AssetReadError)
/// taxonomy instead of merging here; on any of these the screen keeps its
/// placeholder and stays navigable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmbientFailure {
    Workspace,
}

/// Backend-owned resources for active Ambient Home and its shared canvas.
pub struct SharedCache {
    pub sky_pack: Option<AssetBuffer>,
    pub sky_len: usize,
    pub canvas: Option<&'static ExternalL8CanvasBuffer>,
    /// Journey fraction of the last published composition (`None` until the
    /// first compose, including sky-only composes before time is known).
    pub composed_fraction: Option<f32>,
    /// A new screen activation must publish to its own canvas widget even
    /// when its semantic cursor matches the previous activation exactly.
    pub composition_valid: bool,
    /// Mountain coverage of the last published composition (`None` until
    /// the first mountain repaint). Tracked alongside the sun fraction so
    /// a late overlay load publishes even when the sun has not moved, and so
    /// a failed composition can retry without losing the repaint.
    pub composed_mountain_percent: Option<u8>,
    /// Whether the last published composition rendered without the
    /// mountain (climate unavailable). `false` until the first unavailable
    /// transition while a mountain is on screen.
    pub composed_mountain_unavailable: bool,
    /// Overlay presence used for the last published frame. A late
    /// successful load must recompose even if temperature and sun have not
    /// moved.
    pub composed_mountain_loaded: bool,
    /// Latest composed mountain overlay (ink plane then eligibility plane,
    /// [`mountain_assets::OVERLAY_LEN`] bytes), retained while Ambient Home
    /// is active and merged into the screen's 45 KB staging per
    /// composition. Replaced on percent or generation change; never a
    /// whole-pack buffer.
    pub mountain_overlay: Option<LargeByteBuffer>,
    /// Snow percent the retained overlay was composed for.
    mountain_overlay_percent: Option<u8>,
    /// Upload generation the retained overlay was composed under.
    mountain_overlay_generation: u32,
    /// Snow percent the screen currently wants composed (`None` while
    /// unavailable: no climate yet, or climate lost). Refreshed from the
    /// [`Self::service`] argument on every call; the loader queues only
    /// this percent and merges only its overlay.
    mountain_desired_percent: Option<u8>,
    mountain_attempts: u8,
    mountain_next_retry_ms: u64,
    mountain_parked: bool,
    seen_mountain_upload_generation: u32,
    sky_request_id: Option<u32>,
    seen_sky_upload_generation: u32,
    attempts: u8,
    next_retry_ms: u64,
    parked: bool,
}

impl SharedCache {
    pub const fn new() -> Self {
        Self {
            sky_pack: None,
            sky_len: 0,
            canvas: None,
            composed_fraction: None,
            composition_valid: false,
            composed_mountain_percent: None,
            composed_mountain_unavailable: false,
            composed_mountain_loaded: false,
            mountain_overlay: None,
            mountain_overlay_percent: None,
            mountain_overlay_generation: 0,
            mountain_desired_percent: None,
            mountain_attempts: 0,
            mountain_next_retry_ms: 0,
            mountain_parked: false,
            seen_mountain_upload_generation: 0,
            sky_request_id: None,
            seen_sky_upload_generation: 0,
            attempts: 0,
            next_retry_ms: 0,
            parked: false,
        }
    }

    /// Whether the active screen owns a validated sky pack.
    pub fn assets_ready(&self) -> bool {
        self.sky_pack.is_some()
    }

    /// Whether the overlay the screen currently wants is retained: present,
    /// composed for the desired percent, and under the current upload
    /// generation. Sun-only recomposes reuse it with zero SD/CRC work.
    pub fn mountain_ready(&self) -> bool {
        match (
            &self.mountain_overlay,
            self.mountain_overlay_percent,
            self.mountain_desired_percent,
        ) {
            (Some(_), Some(have), Some(want)) => {
                have == want
                    && self.mountain_overlay_generation == self.seen_mountain_upload_generation
            }
            _ => false,
        }
    }

    /// Borrowed overlay for `percent`: `Some` only when the retained
    /// overlay is current-generation and composed for exactly that percent.
    /// Anything else merges nothing — the scene publishes the exact
    /// sky/sun baseline until the matching overlay lands.
    pub fn mountain_overlay_for(&self, percent: u8) -> Option<&[u8]> {
        let overlay = self.mountain_overlay.as_ref()?;
        if self.mountain_overlay_generation != self.seen_mountain_upload_generation {
            return None;
        }
        if self.mountain_overlay_percent != Some(percent) {
            return None;
        }
        Some(overlay.as_slice())
    }

    pub fn decoded(&self) -> Option<::ambient_assets::AmbientAssets<'_>> {
        let pack = self.sky_pack.as_ref()?;
        #[cfg(target_os = "none")]
        {
            let file = pack.part(0)?.get(..self.sky_len)?;
            ::ambient_assets::decode(file).ok()
        }
        #[cfg(not(target_os = "none"))]
        {
            let file = pack.as_slice().get(..self.sky_len)?;
            ::ambient_assets::decode(file).ok()
        }
    }

    /// Whether the loader parked after exhausting its bounded attempts.
    pub fn parked(&self) -> bool {
        self.parked
    }

    /// Monotonic instant the next loader attempt is due (`0` = now). Reads
    /// `u64::MAX` once parked, once the pack is adopted, while no mountain
    /// percent is desired, or once the desired overlay is retained.
    pub fn retry_due_ms(&self) -> u64 {
        let sky_due = if self.parked || self.sky_pack.is_some() {
            u64::MAX
        } else {
            self.next_retry_ms
        };
        let mountain_due = if self.mountain_parked
            || self.mountain_desired_percent.is_none()
            || self.mountain_ready()
        {
            u64::MAX
        } else {
            self.mountain_next_retry_ms
        };
        sky_due.min(mountain_due)
    }

    /// Drive one loader step for the desired mountain percent (which the
    /// screen passes as `None` while unavailable: no climate yet, or
    /// climate lost): adopt a landed matching completion, queue a fresh
    /// render when idle and due, count failures toward parking. No LVGL, no
    /// waiting.
    ///
    /// A committed upload drops the old overlay immediately (the next
    /// composition falls back to the exact sky/sun baseline), drains the
    /// superseded completion, and rearms the loader for the desired
    /// percent. Stale completions (superseded percent or generation) are
    /// dropped without counting a failure and re-request the desired
    /// percent at once; `Busy`/`QueueFull` are retry/poll states, never
    /// counted failures. Other errors keep the bounded 5-attempt/30 s park
    /// behavior.
    pub fn service(&mut self, now_ms: u64, desired_mountain_percent: Option<u8>) {
        let previous_desired = self.mountain_desired_percent;
        self.mountain_desired_percent = desired_mountain_percent;
        let sky_generation = self.sync_sky_generation(now_ms);
        self.sync_mountain_generation(now_ms);
        self.rearm_on_desired_transition(now_ms, previous_desired);
        self.drain_mountain_completion(now_ms);
        self.drain_sky_completion(now_ms, sky_generation);
        self.pump_sky_request(now_ms);
        self.pump_mountain_request(now_ms);
    }

    fn sync_sky_generation(&mut self, now_ms: u64) -> u32 {
        let sky_generation = upload_generation();
        if sky_generation != self.seen_sky_upload_generation {
            self.seen_sky_upload_generation = sky_generation;
            if let Some(pack) = self.sky_pack.take() {
                drop(pack);
            }
            self.sky_len = 0;
            self.composition_valid = false;
            self.attempts = 0;
            self.parked = false;
            self.next_retry_ms = now_ms;
            drop(take_assets());
            console::println!("AMBIENT_ASSETS status=generation_invalidated");
        }
        sky_generation
    }

    fn sync_mountain_generation(&mut self, now_ms: u64) {
        let upload_generation = mountain_assets::upload_generation();
        if upload_generation != self.seen_mountain_upload_generation {
            crate::firmware::ui::clear_ambient_mountain_status();
            self.seen_mountain_upload_generation = upload_generation;
            if let Some(overlay) = self.mountain_overlay.take() {
                console::println!(
                    "AMBIENT_MOUNTAIN status=generation_invalidated bytes={}",
                    overlay.as_slice().len()
                );
                drop(overlay);
            }
            self.mountain_overlay_percent = None;
            // A completion queued before the commit describes the old
            // file. Drop it before requesting the newly committed render.
            drop(mountain_assets::take_assets());
            self.mountain_attempts = 0;
            self.mountain_parked = false;
            self.mountain_next_retry_ms = now_ms;
        }
    }

    fn rearm_on_desired_transition(&mut self, now_ms: u64, previous_desired: Option<u8>) {
        // Explicit desired-percent transitions: a percent change, or an
        // unavailable recovery whose retained overlay is not already exactly
        // the current generation composed for the recovered percent, drops
        // the stale overlay and rearms the loader at once. A matching
        // retained overlay survives the unavailable round-trip; losing the
        // desired percent retains the old overlay and queues nothing.
        let retained = match (&self.mountain_overlay, self.mountain_overlay_percent) {
            (Some(_), Some(percent)) => Some((percent, self.mountain_overlay_generation)),
            _ => None,
        };
        if mountain_assets::transition_requires_rearm(
            previous_desired,
            self.mountain_desired_percent,
            retained,
            self.seen_mountain_upload_generation,
        ) {
            crate::firmware::ui::clear_ambient_mountain_status();
            let dropped = self
                .mountain_overlay
                .take()
                .map(|overlay| overlay.as_slice().len());
            self.mountain_overlay_percent = None;
            self.mountain_attempts = 0;
            self.mountain_parked = false;
            self.mountain_next_retry_ms = now_ms;
            console::println!(
                "AMBIENT_MOUNTAIN status=desired_transition dropped_bytes={}",
                dropped.unwrap_or(0)
            );
        }
    }

    fn drain_mountain_completion(&mut self, now_ms: u64) {
        if let Some(completion) = mountain_assets::take_assets() {
            if !mountain_assets::completion_is_usable(
                completion.generation,
                completion.percent,
                self.seen_mountain_upload_generation,
                self.mountain_desired_percent,
            ) {
                console::println!(
                    "AMBIENT_MOUNTAIN status=stale_completion id={} percent={} generation={}",
                    completion.id,
                    completion.percent,
                    completion.generation
                );
                drop(completion);
                self.mountain_next_retry_ms = now_ms;
            } else {
                match completion.result {
                    Ok(buffer) => {
                        self.settle_mountain_buffer(
                            completion.id,
                            completion.percent,
                            completion.generation,
                            buffer,
                            now_ms,
                        );
                    }
                    Err(AssetReadError::Busy | AssetReadError::QueueFull) => {
                        // The SD worker answered with a retry/poll state
                        // (upload active, or a superseded request): keep
                        // polling, count nothing.
                        self.mountain_next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                    }
                    Err(error) => {
                        console::println!(
                            "AMBIENT_MOUNTAIN status=load_error err={:?} retryable={}",
                            error,
                            error.is_retryable()
                        );
                        self.note_mountain_failure(now_ms);
                    }
                }
            }
        }
    }

    fn settle_mountain_buffer(
        &mut self,
        id: u32,
        percent: u8,
        generation: u32,
        buffer: LargeByteBuffer,
        now_ms: u64,
    ) {
        if buffer.as_slice().len() == mountain_assets::OVERLAY_LEN {
            self.adopt_mountain_overlay(id, percent, generation, buffer);
        } else {
            console::println!("AMBIENT_MOUNTAIN status=stale_bytes");
            drop(buffer);
            self.note_mountain_failure(now_ms);
        }
    }

    fn drain_sky_completion(&mut self, now_ms: u64, sky_generation: u32) {
        if let Some(completion) = take_assets() {
            if Some(completion.id) != self.sky_request_id
                || completion.generation != sky_generation
                || self.sky_pack.is_some()
            {
                console::println!(
                    "AMBIENT_ASSETS status=stale_completion id={} generation={}",
                    completion.id,
                    completion.generation
                );
                drop(completion);
                self.next_retry_ms = now_ms;
            } else {
                self.sky_request_id = None;
                match completion.result {
                    Ok(buffer) => {
                        self.settle_sky_buffer(completion.id, buffer, completion.len, now_ms);
                    }
                    Err(AssetReadError::Busy | AssetReadError::QueueFull) => {
                        self.next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                    }
                    Err(error) => {
                        console::println!(
                            "AMBIENT_ASSETS status=load_error err={:?} retryable={}",
                            error,
                            error.is_retryable()
                        );
                        self.note_failure(now_ms);
                    }
                }
            }
        }
    }

    fn settle_sky_buffer(&mut self, id: u32, buffer: AssetBuffer, len: usize, now_ms: u64) {
        #[cfg(target_os = "none")]
        let usable = buffer
            .part(0)
            .filter(|part| part.len() == len)
            .is_some_and(|part| ::ambient_assets::decode(part).is_ok());
        #[cfg(not(target_os = "none"))]
        let usable =
            len == buffer.as_slice().len() && ::ambient_assets::decode(buffer.as_slice()).is_ok();
        if usable {
            self.adopt_asset_buffer(id, buffer, len);
        } else {
            console::println!("AMBIENT_ASSETS status=stale_bytes");
            drop(buffer);
            self.note_failure(now_ms);
        }
    }

    fn pump_sky_request(&mut self, now_ms: u64) {
        if self.sky_pack.is_none() && !self.parked && now_ms >= self.next_retry_ms {
            match request_assets() {
                Ok(id) => {
                    self.sky_request_id = Some(id);
                    // Read accepted: reap the completion on a short cadence.
                    // A wedged worker only costs a channel poll per tick.
                    self.next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                }
                Err(AssetReadError::Busy | AssetReadError::QueueFull) => {
                    // A read is already outstanding (ours or nothing we can
                    // preempt): keep polling for its completion, count
                    // nothing.
                    self.next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                }
                Err(error) => {
                    console::println!("AMBIENT_ASSETS status=request_error err={:?}", error);
                    self.note_failure(now_ms);
                }
            }
        }
    }

    fn pump_mountain_request(&mut self, now_ms: u64) {
        let upload_mode_enabled = {
            #[cfg(feature = "asset-upload-http")]
            {
                service_mode::upload_transfers_enabled()
            }
            #[cfg(not(feature = "asset-upload-http"))]
            {
                false
            }
        };
        if self.mountain_desired_percent.is_some()
            && !self.mountain_ready()
            && !self.mountain_parked
            && now_ms >= self.mountain_next_retry_ms
        {
            if upload_mode_enabled {
                self.mountain_next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                return;
            }
            let percent = self
                .mountain_desired_percent
                .expect("mountain render just gated on a desired percent");
            match mountain_assets::request_assets(percent) {
                Ok(()) | Err(AssetReadError::Busy | AssetReadError::QueueFull) => {
                    self.mountain_next_retry_ms = now_ms.saturating_add(COMPLETION_POLL_MS);
                }
                Err(error) => {
                    console::println!("AMBIENT_MOUNTAIN status=request_error err={:?}", error);
                    self.note_mountain_failure(now_ms);
                }
            }
        }
    }

    fn note_mountain_failure(&mut self, now_ms: u64) {
        self.mountain_attempts = self.mountain_attempts.saturating_add(1);
        if self.mountain_attempts >= MAX_ATTEMPTS {
            self.mountain_parked = true;
            self.mountain_next_retry_ms = u64::MAX;
            console::println!(
                "AMBIENT_MOUNTAIN status=parked attempts={}",
                self.mountain_attempts
            );
        } else {
            self.mountain_next_retry_ms = now_ms.saturating_add(RETRY_INTERVAL_MS);
        }
    }

    fn note_failure(&mut self, now_ms: u64) {
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= MAX_ATTEMPTS {
            self.parked = true;
            self.next_retry_ms = u64::MAX;
            console::println!("AMBIENT_ASSETS status=parked attempts={}", self.attempts);
        } else {
            self.next_retry_ms = now_ms.saturating_add(RETRY_INTERVAL_MS);
        }
    }

    /// Adopt one matching overlay buffer. Called only after [`Self::service`]
    /// confirmed the completion echoes the desired percent and the current
    /// generation and the buffer holds exactly [`mountain_assets::OVERLAY_LEN`]
    /// bytes. A usable completion replaces any old overlay (never rejected
    /// as a duplicate) and resets the retry budget: attempts and parked
    /// state clear and the retry instant parks at non-due while the desired
    /// overlay is retained.
    fn adopt_mountain_overlay(
        &mut self,
        id: u32,
        percent: u8,
        generation: u32,
        buffer: LargeByteBuffer,
    ) {
        let replaced = self.mountain_overlay.is_some();
        if let Some(old) = self.mountain_overlay.replace(buffer) {
            drop(old);
        }
        self.mountain_overlay_percent = Some(percent);
        self.mountain_overlay_generation = generation;
        crate::firmware::ui::mark_ambient_mountain_adopted();
        self.mountain_attempts = 0;
        self.mountain_parked = false;
        self.mountain_next_retry_ms = u64::MAX;
        console::println!(
            "AMBIENT_MOUNTAIN status=adopted id={} percent={} generation={} bytes={} replaced={}",
            id,
            percent,
            generation,
            mountain_assets::OVERLAY_LEN,
            replaced as u8,
        );
    }

    pub fn release_inactive_assets(&mut self) {
        crate::firmware::ui::clear_ambient_mountain_status();
        mountain_assets::release_resident_pack();
        self.composition_valid = false;
        if let Some(pack) = self.sky_pack.take() {
            #[cfg(target_os = "none")]
            let bytes = pack.part(0).map(|part| part.len()).unwrap_or(0);
            #[cfg(not(target_os = "none"))]
            let bytes = pack.as_slice().len();
            console::println!("AMBIENT_ASSETS status=released bytes={}", bytes);
            drop(pack);
        }
        self.sky_len = 0;
        self.sky_request_id = None;
        self.attempts = 0;
        self.next_retry_ms = 0;
        self.parked = false;
        if let Some(overlay) = self.mountain_overlay.take() {
            console::println!(
                "AMBIENT_MOUNTAIN status=released bytes={}",
                overlay.as_slice().len()
            );
            drop(overlay);
        }
        self.mountain_overlay_percent = None;
        self.mountain_desired_percent = None;
        self.mountain_attempts = 0;
        self.mountain_next_retry_ms = 0;
        self.mountain_parked = false;
        self.composed_mountain_loaded = false;
        drop(mountain_assets::take_assets());
        drop(take_assets());
    }

    fn adopt_asset_buffer(&mut self, id: u32, buffer: AssetBuffer, len: usize) {
        if self.sky_pack.is_none() {
            self.sky_len = len;
            self.sky_pack = Some(buffer);
            console::println!("AMBIENT_ASSETS status=adopted id={} bytes={}", id, len);
        }
    }

    /// Lazily allocate and leak the 360 KB L8 publish canvas once per boot.
    /// Strict PSRAM: an internal-memory fallback is rejected, not kept, and
    /// the 4-byte adapter alignment is verified on the owned buffer *before*
    /// it is leaked, so a rejection frees the buffer instead of pinning it.
    /// Callers allocate the small keeper wrapper *before* calling this, so a
    /// wrapper OOM never costs the large buffer and retries cleanly.
    pub fn canvas_or_allocate(&mut self) -> Result<&'static mut [u8], AmbientFailure> {
        if self.canvas.is_some() {
            return Err(AmbientFailure::Workspace);
        }
        let buffer =
            alloc_large_byte_buffer(CANVAS_PIXELS).map_err(|_| AmbientFailure::Workspace)?;
        if buffer.placement() != BufferPlacement::Psram {
            return Err(AmbientFailure::Workspace);
        }
        if !(buffer.as_slice().as_ptr() as usize).is_multiple_of(4) {
            return Err(AmbientFailure::Workspace);
        }
        Ok(buffer.into_static_mut_slice())
    }

    /// Lazily adopt the boot canvas keeper. Mirrors the analog clock's
    /// keeper-before-buffer order: a wrapper OOM loses nothing and the next
    /// entry retries cleanly instead of pinning a permanent failure.
    pub fn canvas_or_keep(&mut self) -> Result<&'static ExternalL8CanvasBuffer, AmbientFailure> {
        if let Some(canvas) = self.canvas {
            return Ok(canvas);
        }
        let keeper: ExternalValue<Option<ExternalL8CanvasBuffer>> =
            ExternalValue::try_new_with(|| None).map_err(|_| {
                console::println!("AMBIENT_CANVAS status=keeper_oom");
                AmbientFailure::Workspace
            })?;
        let storage = self.canvas_or_allocate()?;
        // Storage was placement- and alignment-verified before leaking, so
        // the adapter constructor cannot fail on those grounds (it only
        // rejects empty/misaligned storage).
        let adopted = ExternalL8CanvasBuffer::new(storage).map_err(|_| {
            console::println!("AMBIENT_CANVAS status=adopt_failed");
            AmbientFailure::Workspace
        })?;
        let slot: &'static mut Option<ExternalL8CanvasBuffer> = keeper.leak();
        *slot = Some(adopted);
        self.canvas = Some(slot.as_ref().expect("ambient canvas keeper just installed"));
        Ok(self.canvas.expect("ambient canvas just installed"))
    }
}
