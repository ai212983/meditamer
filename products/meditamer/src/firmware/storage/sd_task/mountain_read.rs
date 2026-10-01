#![deny(unsafe_code)]

//! Bounded mountain-overlay renders with a cached per-generation session.
//!
//! Serves one queued [`MountainReadRequest`](super::super::mountain_assets::MountainReadRequest)
//! by composing the requested snow percent into exactly one strict-PSRAM
//! [`OVERLAY_LEN`](super::super::mountain_assets::OVERLAY_LEN)-byte overlay
//! (ink plane then eligibility plane, per the ambient composer constants)
//! with an optional, screen-scoped segmented PSRAM copy of the v1 pack.
//!
//! Scheduling contract (see `mountain_assets`): never interleave with an
//! active upload session (respond [`AssetReadError::Busy`] instead, which
//! keeps the upload session and engine valid), answer a request a committed
//! upload superseded without touching SD or stale session metadata, never
//! call the disabled service-upload handler, and never cancel an in-flight
//! SD/DMA read with an external timeout. Transport init owns its
//! cooperative deadline; the reads themselves are bounded by the
//! service/transport timeouts. On transport failure the card probe and
//! shared engine are invalidated; the normal idle power-off path is
//! untouched.
//!
//! [`MountainSession`] is owned by the SD task runtime (inside the existing
//! `ExternalValue`-boxed runner future — never a static) and threaded only
//! into this handler. The first request of a generation validates once: a
//! [`FatRequest::Stream`] of the exact file captures the 64-byte header and
//! incrementally CRCs only payload bytes, requiring the exact
//! [`MOUNTAIN_FILE_LEN`](super::super::mountain_assets::MOUNTAIN_FILE_LEN),
//! the frozen first-row/row-count/file-length header fields, and a CRC
//! match. If a strict-external segmented bundle was admitted, the same
//! stream fills it, histogram and renders read only PSRAM, and screen exit
//! releases it. Allocation failure keeps the existing bounded SD-range path. Neither
//! path publishes a session before validation and histogram completion.
//!
//! All bulk arrays here (the row scratch, the two histogram row buffers,
//! the 64-byte header capture) are async-handler locals inside the already
//! external boxed runner future; no new large static and no channel
//! payload-by-value is added.

use embassy_time::{Duration, Instant};
use phase_arena::{PartRequest, PartShape, RuntimeBundle};

use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatPayloadId, FatRequest, FatResult, FatStageLabel,
    SdFatError,
};
use sdcard::service::{FatBuffers, FatObserver};
use sdcard::transport::TransportError;
use sdcard::SD_PATH_MAX;

use super::super::super::psram::{
    acquire_runtime_bundle, alloc_large_byte_buffer, BufferPlacement, LargeByteBuffer,
    RuntimeBundleAcquireError,
};
#[cfg(feature = "asset-upload-http")]
use super::super::super::service_mode;
use super::super::super::types::{SdPowerRequest, SdProbeDriver};
use super::super::mountain_assets::{
    self as mountain, AssetReadError, MountainReadCompletion, MountainReadRequest,
};
use super::engine_driver::{run_fat_request, ProductObserver};
use super::request_sd_power;

// Cooperative init deadline shared with normal requests: `SdCardProbe`
// checks it only between completed DMA transfers, so expiry never cancels an
// in-flight DMA.
const ASSET_INIT_DEADLINE_MS: u64 = 8_000;
// Candidate geometry, never treated as a shared-heap capacity guarantee:
// allocation is fallible and the established bounded reader remains available.
const RESIDENT_SEGMENT_BYTES: usize = 32 * 1024;
const RESIDENT_PART: PartRequest = PartRequest {
    bytes: mountain::MOUNTAIN_FILE_LEN,
    align: 1,
    shape: PartShape::Segmented {
        max_segment_bytes: RESIDENT_SEGMENT_BYTES,
    },
};

struct ResidentPack {
    bytes: RuntimeBundle<esp_alloc::ExternalMemory>,
}

/// Cached per-generation validation metadata for the mountain pack.
///
/// Neither the session nor the histogram is `Copy`/`Clone`: the histogram
/// is caller-owned bulk state that must move with its generation, never be
/// duplicated across generations.
pub(super) struct MountainSession {
    generation: u32,
    header: mountain_snow::pack::Header,
    histogram: [u32; 256],
    eligible_count: u32,
    resident: Option<ResidentPack>,
}

impl MountainSession {
    pub(super) fn resident_bytes(&self) -> usize {
        self.resident
            .as_ref()
            .map_or(0, |pack| pack.bytes.total_bytes())
    }
}

#[derive(Default)]
struct MountainTiming {
    stream_us: u64,
    histogram_read_us: u64,
    histogram_compute_us: u64,
    render_read_us: u64,
    render_compose_us: u64,
    histogram_reads: u32,
    render_reads: u32,
    resident_bytes: usize,
}

fn elapsed_us(started: Instant) -> u64 {
    started.elapsed().as_micros()
}

/// Serve one queued mountain overlay render to completion and publish the
/// owned result. Never blocks: the result channel `try_send` cannot stall
/// the SD owner.
pub(super) async fn serve_mountain_asset_read(
    request: MountainReadRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
    session: &mut Option<MountainSession>,
) {
    let started = Instant::now();
    let mut timing = MountainTiming::default();
    let result = run_mountain_overlay(
        request,
        sd_probe,
        powered,
        upload_active,
        fat_engine,
        session,
        &mut timing,
    )
    .await;
    console::println!(
        "MOUNTAIN_TIMING id={} generation={} total_us={} stream_us={} histogram_read_us={} histogram_compute_us={} histogram_reads={} render_read_us={} render_compose_us={} render_reads={} resident_bytes={} ok={}",
        request.id,
        request.generation,
        elapsed_us(started),
        timing.stream_us,
        timing.histogram_read_us,
        timing.histogram_compute_us,
        timing.histogram_reads,
        timing.render_read_us,
        timing.render_compose_us,
        timing.render_reads,
        timing.resident_bytes,
        result.is_ok(),
    );
    match &result {
        Ok(_) => console::println!(
            "sdtask: mountain_overlay_ok id={} percent={} generation={}",
            request.id,
            request.percent,
            request.generation
        ),
        Err(err) => console::println!(
            "sdtask: mountain_overlay_error id={} percent={} generation={} err={:?} retryable={}",
            request.id,
            request.percent,
            request.generation,
            err,
            err.is_retryable()
        ),
    }
    mountain::complete_asset_read(MountainReadCompletion {
        id: request.id,
        percent: request.percent,
        generation: request.generation,
        result,
    });
}

async fn run_mountain_overlay(
    request: MountainReadRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
    session: &mut Option<MountainSession>,
    timing: &mut MountainTiming,
) -> Result<LargeByteBuffer, AssetReadError> {
    // The request channel is crate-internal and `request_assets` already
    // rejects percents above 100, but fail fast here too: never power the
    // card or touch the session for a percent no composer accepts.
    if request.percent > 100 {
        return Err(AssetReadError::BadHeader);
    }
    if !mountain::resident_wanted() {
        return Err(AssetReadError::Busy);
    }
    // Upload mode owns SD/radio admission even between individual HTTP
    // requests. Defer this optional, long-running render until transfers are
    // disabled; otherwise a mode transition can begin while the non-cancellable
    // range batch owns the card and leave the next upload unable to re-init.
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
    if upload_active || upload_mode_enabled {
        return Err(AssetReadError::Busy);
    }
    // A committed upload between queue and serve supersedes this request.
    // Answer without touching SD or stale session metadata; `Busy` is a
    // retry/poll state for the UI, never a counted failure.
    if !mountain::request_is_current(request.generation, mountain::upload_generation()) {
        return Err(AssetReadError::Busy);
    }
    if !*powered {
        if !request_sd_power(SdPowerRequest::On).await {
            return Err(AssetReadError::Unavailable);
        }
        *powered = true;
    }
    if !sd_probe.is_initialized() {
        let deadline = Instant::now() + Duration::from_millis(ASSET_INIT_DEADLINE_MS);
        if let Err(err) = sd_probe.init_until(deadline).await {
            sd_probe.recover_after_timeout();
            fat_engine.invalidate();
            console::println!("sdtask: mountain_overlay_init_error err={:?}", err);
            return Err(AssetReadError::Unavailable);
        }
    }

    // The ESP32 async SPI-DMA completion future can lose a wake during this
    // exceptionally long sequence of short random reads. Scope esp-hal's
    // supported CPU/FIFO fallback to this job only; all established
    // boot/Clock/upload paths continue using DMA.
    sd_probe
        .set_bounded_cpu_transfers(true)
        .map_err(|_| AssetReadError::Unavailable)?;
    let result = run_mountain_overlay_io(request, sd_probe, fat_engine, session, timing).await;
    if sd_probe.set_bounded_cpu_transfers(false).is_err() {
        sd_probe.recover_after_timeout();
        fat_engine.invalidate();
        *session = None;
        return Err(AssetReadError::Unavailable);
    }
    result
}

async fn run_mountain_overlay_io(
    request: MountainReadRequest,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
    session: &mut Option<MountainSession>,
    timing: &mut MountainTiming,
) -> Result<LargeByteBuffer, AssetReadError> {
    // Validate once per generation; reuse the cached session otherwise. A
    // failed validation keeps the previous generation's session (never
    // relabeled), and the error below answers this request.
    let reusable = mountain::session_reusable(
        session.as_ref().map(|cached| cached.generation),
        request.generation,
        mountain::upload_generation(),
    );
    if !reusable {
        let validated = validate_mountain_file(sd_probe, fat_engine, timing).await?;
        console::println!(
            "MOUNTAIN_STREAM status=validated generation={}",
            request.generation
        );
        *session = Some(MountainSession {
            generation: request.generation,
            header: validated.header,
            histogram: validated.histogram,
            eligible_count: validated.eligible_count,
            resident: validated.resident,
        });
    } else {
        console::println!(
            "MOUNTAIN_STREAM status=validation_reused generation={}",
            request.generation
        );
    }
    let cached = session.as_ref().ok_or(AssetReadError::Unavailable)?;
    if cached.generation != request.generation {
        // Unreachable: the session is cached only on successful validation
        // of this request's generation above, and uploads (the only
        // generation source) cannot interleave this handler.
        return Err(AssetReadError::Unavailable);
    }
    match render_overlay(request.percent, cached, sd_probe, fat_engine, timing).await {
        Err(
            err
            @ (AssetReadError::BadSize | AssetReadError::NotFound | AssetReadError::Unavailable),
        ) => {
            // The cached histogram/offsets no longer describe the file:
            // drop the session so the next same-generation request
            // revalidates instead of repeating the stale ranges.
            *session = None;
            Err(err)
        }
        result => result,
    }
}

/// Validated file content for one generation: the frozen header plus the
/// barrier histogram the cut derives from. Built once, then moved into the
/// cached [`MountainSession`].
struct ValidatedMountain {
    header: mountain_snow::pack::Header,
    histogram: [u32; 256],
    eligible_count: u32,
    resident: Option<ResidentPack>,
}

/// Stream the exact file once, requiring the frozen header fields and CRC,
/// then build the histogram from resident PSRAM when allocation succeeded,
/// or through the established bounded SD ranges otherwise. The caller
/// caches only the fully validated result.
async fn validate_mountain_file(
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
    timing: &mut MountainTiming,
) -> Result<ValidatedMountain, AssetReadError> {
    use mountain_snow::pack::WIDTH;

    let (path, path_len) = mountain::asset_path_field();
    let mut output = [];
    let mut resident = match acquire_runtime_bundle(&[RESIDENT_PART]) {
        Ok(bundle) => Some(bundle),
        Err(RuntimeBundleAcquireError::AllocationFailed(_)) => {
            console::println!("MOUNTAIN_STREAM status=resident_alloc_failed fallback=ranges");
            None
        }
        Err(RuntimeBundleAcquireError::AllocatorNotReady) => {
            return Err(AssetReadError::Unavailable);
        }
        Err(RuntimeBundleAcquireError::InvalidRequest(_)) => return Err(AssetReadError::BadSize),
    };
    let mut observer = MountainStreamObserver::new(resident.as_mut());
    let stream_started = Instant::now();
    let read_result = sdcard::service::run_fat_request(
        FatRequest::Stream { path, path_len },
        sd_probe,
        fat_engine,
        &mut FatBuffers {
            input: &[],
            output: &mut output,
        },
        &mut observer,
    )
    .await;
    timing.stream_us = elapsed_us(stream_started);
    let header_bytes = observer.header;
    let seen = observer.seen;
    let stream_invalid = observer.invalid || seen != mountain::MOUNTAIN_FILE_LEN;
    let crc = observer.crc_finalize();
    drop(observer);
    // Transport errors first: a mid-stream transport failure must recover
    // and invalidate the probe/engine and report `Unavailable`, even when
    // the partial stream also tripped the observer's gap/overflow flag.
    match read_result {
        FatResult::Streamed { bytes } if bytes as usize == mountain::MOUNTAIN_FILE_LEN => {}
        FatResult::Streamed { bytes } => {
            console::println!(
                "sdtask: mountain_overlay_truncated bytes={} want={}",
                bytes,
                mountain::MOUNTAIN_FILE_LEN
            );
            return Err(AssetReadError::BadSize);
        }
        FatResult::Error(err) => {
            if err.is_transport_failure() {
                sd_probe.recover_after_timeout();
                fat_engine.invalidate();
                return Err(AssetReadError::Unavailable);
            }
            if matches!(err, FatEngineError::Cancelled) {
                return Err(AssetReadError::BadSize);
            }
            return Err(map_engine_error(&err));
        }
        _ => return Err(AssetReadError::BadSize),
    }
    if stream_invalid {
        return Err(AssetReadError::BadSize);
    }
    let validated =
        mountain_snow::source::ValidatedPack::from_streamed_crc(&header_bytes, seen, crc)
            .map_err(map_pack_error)?;
    let header = validated.header();
    if header.first_row != mountain::MOUNTAIN_FIRST_ROW
        || header.rows != mountain::MOUNTAIN_ROWS
        || header.file_len() != mountain::MOUNTAIN_FILE_LEN
    {
        return Err(AssetReadError::BadHeader);
    }
    // Row indices are loop-bounded, so `row_ranges` cannot fail.
    let mut histogram = [0u32; 256];
    let mut eligible_count = 0u32;
    let mut barrier = [0u8; WIDTH];
    let mut eligible = [0u8; WIDTH / 8];
    for row in 0..header.rows {
        let ranges = header.row_ranges(row).map_err(map_pack_error)?;
        let read_started = Instant::now();
        if let Some(bundle) = resident.as_ref() {
            bundle
                .read_at(0, ranges.barrier.offset, &mut barrier)
                .map_err(|_| AssetReadError::BadSize)?;
            bundle
                .read_at(0, ranges.eligible.offset, &mut eligible)
                .map_err(|_| AssetReadError::BadSize)?;
        } else {
            read_exact_range(
                path,
                path_len,
                ranges.barrier.offset,
                &mut barrier,
                sd_probe,
                fat_engine,
            )
            .await?;
            timing.histogram_reads += 1;
            read_exact_range(
                path,
                path_len,
                ranges.eligible.offset,
                &mut eligible,
                sd_probe,
                fat_engine,
            )
            .await?;
            timing.histogram_reads += 1;
        }
        timing.histogram_read_us += elapsed_us(read_started);
        let compute_started = Instant::now();
        for x in 0..WIDTH {
            if eligible[x / 8] & (0x80 >> (x % 8)) != 0 {
                histogram[barrier[x] as usize] = histogram[barrier[x] as usize].saturating_add(1);
                eligible_count = eligible_count.saturating_add(1);
            }
        }
        timing.histogram_compute_us += elapsed_us(compute_started);
    }
    timing.resident_bytes = resident.as_ref().map_or(0, RuntimeBundle::total_bytes);
    if let Some(bundle) = resident.as_ref() {
        console::println!(
            "MOUNTAIN_CACHE status=validated bytes={} chunks={}",
            bundle.total_bytes(),
            bundle.segment_count(0).map_or(0, |count| count),
        );
    }
    Ok(ValidatedMountain {
        header,
        histogram,
        eligible_count,
        resident: resident.map(|bytes| ResidentPack { bytes }),
    })
}

/// Compose from resident segmented bytes when present; retain the established
/// five-SD-range-per-row path only for allocation fallback.
async fn render_overlay(
    percent: u8,
    cached: &MountainSession,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
    timing: &mut MountainTiming,
) -> Result<LargeByteBuffer, AssetReadError> {
    let header = cached.header;
    let target = mountain::coverage_target(cached.eligible_count, percent);
    let cut = mountain_snow::composer::cut_from_histogram(&cached.histogram, target);

    let mut overlay =
        alloc_large_byte_buffer(mountain::OVERLAY_LEN).map_err(|_| AssetReadError::OutOfMemory)?;
    if overlay.placement() != BufferPlacement::Psram {
        return Err(AssetReadError::NotPsram);
    }
    let (path, path_len) = mountain::asset_path_field();
    let mut scratch = mountain_snow::source::RowScratch::new();
    let plane = mountain::OVERLAY_PLANE_BYTES;
    let row_bytes = mountain::OVERLAY_ROW_BYTES;
    for row in 0..header.rows {
        // `row` is loop-bounded and the header is frozen, so every range is
        // in-band and every row slice lands exactly inside its plane.
        let read_started = Instant::now();
        if let Some(resident) = cached.resident.as_ref() {
            let ranges = header.row_ranges(row).map_err(map_pack_error)?;
            resident
                .bytes
                .read_at(0, ranges.rock.offset, &mut scratch.rock)
                .map_err(|_| AssetReadError::BadSize)?;
            resident
                .bytes
                .read_at(0, ranges.snow.offset, &mut scratch.snow)
                .map_err(|_| AssetReadError::BadSize)?;
            resident
                .bytes
                .read_at(0, ranges.barrier.offset, &mut scratch.barrier)
                .map_err(|_| AssetReadError::BadSize)?;
            resident
                .bytes
                .read_at(0, ranges.eligible.offset, &mut scratch.eligible)
                .map_err(|_| AssetReadError::BadSize)?;
            resident
                .bytes
                .read_at(0, ranges.noise.offset, &mut scratch.noise)
                .map_err(|_| AssetReadError::BadSize)?;
        } else {
            let ranges = header.row_ranges(row).map_err(map_pack_error)?;
            read_exact_range(
                path,
                path_len,
                ranges.rock.offset,
                &mut scratch.rock,
                sd_probe,
                fat_engine,
            )
            .await?;
            read_exact_range(
                path,
                path_len,
                ranges.snow.offset,
                &mut scratch.snow,
                sd_probe,
                fat_engine,
            )
            .await?;
            read_exact_range(
                path,
                path_len,
                ranges.barrier.offset,
                &mut scratch.barrier,
                sd_probe,
                fat_engine,
            )
            .await?;
            read_exact_range(
                path,
                path_len,
                ranges.eligible.offset,
                &mut scratch.eligible,
                sd_probe,
                fat_engine,
            )
            .await?;
            read_exact_range(
                path,
                path_len,
                ranges.noise.offset,
                &mut scratch.noise,
                sd_probe,
                fat_engine,
            )
            .await?;
            timing.render_reads += 5;
        }
        timing.render_read_us += elapsed_us(read_started);
        let compose_started = Instant::now();
        let frame = overlay.as_mut_slice();
        let (ink_plane, opacity_plane) = frame.split_at_mut(plane);
        let ink_row = &mut ink_plane[row * row_bytes..(row + 1) * row_bytes];
        mountain_snow::composer::compose_row_for_percent(
            &scratch.inputs(),
            percent,
            cut,
            64,
            ink_row,
        )
        .map_err(map_compose_error)?;
        opacity_plane[row * row_bytes..(row + 1) * row_bytes].copy_from_slice(&scratch.eligible);
        timing.render_compose_us += elapsed_us(compose_started);
    }
    console::println!(
        "MOUNTAIN_STREAM status=rendered percent={} rows={}",
        percent,
        header.rows,
    );
    Ok(overlay)
}

/// Read exactly `dest.len()` bytes at `offset` through one
/// [`FatRequest::ReadRange`]. A short read means the file changed under the
/// validated length, so it reports [`AssetReadError::BadSize`]; a missing
/// file reports [`AssetReadError::NotFound`]. Only transport failures
/// invalidate the probe/engine.
async fn read_exact_range(
    path: [u8; SD_PATH_MAX],
    path_len: u8,
    offset: usize,
    dest: &mut [u8],
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Result<(), AssetReadError> {
    let offset = u32::try_from(offset).map_err(|_| AssetReadError::BadSize)?;
    let len = u32::try_from(dest.len()).map_err(|_| AssetReadError::BadSize)?;
    let request = FatRequest::ReadRange {
        path,
        path_len,
        offset,
        len,
        output: FatPayloadId::Primary,
        output_capacity: len,
    };
    match run_fat_request(request, sd_probe, fat_engine, &[], dest).await {
        FatResult::Read { bytes } if bytes as usize == dest.len() => Ok(()),
        FatResult::Read { .. } => Err(AssetReadError::BadSize),
        FatResult::Error(err) => {
            if err.is_transport_failure() {
                sd_probe.recover_after_timeout();
                fat_engine.invalidate();
                return Err(AssetReadError::Unavailable);
            }
            Err(map_engine_error(&err))
        }
        _ => Err(AssetReadError::BadSize),
    }
}

/// Stream observer for validation: captures the first 64 bytes for
/// header parsing and incrementally CRCs only payload bytes. Chunks arrive
/// in file order; any gap, reorder, or byte past the exact file length
/// invalidates the stream (and cancels it early).
struct MountainStreamObserver<'a> {
    header: [u8; mountain_snow::pack::HEADER_LEN],
    seen: usize,
    invalid: bool,
    crc: mountain_snow::pack::Crc32,
    bundle: Option<&'a mut RuntimeBundle<esp_alloc::ExternalMemory>>,
    product: ProductObserver,
}

impl<'a> MountainStreamObserver<'a> {
    fn new(bundle: Option<&'a mut RuntimeBundle<esp_alloc::ExternalMemory>>) -> Self {
        Self {
            header: [0; mountain_snow::pack::HEADER_LEN],
            seen: 0,
            invalid: false,
            crc: mountain_snow::pack::Crc32::new(),
            bundle,
            product: ProductObserver,
        }
    }

    fn crc_finalize(&self) -> u32 {
        self.crc.finalize()
    }
}

impl FatObserver for MountainStreamObserver<'_> {
    fn on_stream_chunk(&mut self, offset: u32, bytes: &[u8]) {
        use mountain_snow::pack::HEADER_LEN;

        let offset = offset as usize;
        let end = offset.saturating_add(bytes.len());
        if offset != self.seen || end > mountain::MOUNTAIN_FILE_LEN {
            self.invalid = true;
            return;
        }
        if self
            .bundle
            .as_mut()
            .is_some_and(|bundle| bundle.write_at(0, offset, bytes).is_err())
        {
            self.invalid = true;
            return;
        }
        self.seen = end;
        if offset < HEADER_LEN {
            let take = (HEADER_LEN - offset).min(bytes.len());
            self.header[offset..offset + take].copy_from_slice(&bytes[..take]);
        }
        let payload_start = HEADER_LEN.saturating_sub(offset).min(bytes.len());
        self.crc.update(&bytes[payload_start..]);
    }
    fn on_stage(&mut self, stage: FatStageLabel, before: bool) {
        self.product.on_stage(stage, before);
    }
    fn on_transport_error(
        &mut self,
        stage: FatStageLabel,
        action: FatIoAction,
        error: TransportError,
    ) {
        self.product.on_transport_error(stage, action, error);
    }
    fn on_complete(&mut self, result: &FatResult) {
        self.product.on_complete(result);
    }
    fn should_cancel(&self) -> bool {
        self.invalid
    }
}

fn map_engine_error(err: &FatEngineError) -> AssetReadError {
    match err {
        FatEngineError::Fat(SdFatError::NotFound) => AssetReadError::NotFound,
        FatEngineError::Fat(
            SdFatError::BufferTooSmall { .. } | SdFatError::RangeBeyondEof { .. },
        ) => AssetReadError::BadSize,
        // Deterministic on-disk FAT structure failures: a retry cannot heal
        // a corrupt boot sector or cluster chain, so they report the
        // existing non-retryable format error rather than `Unavailable`.
        // Engine lifecycle and transport-like errors stay `Unavailable`.
        FatEngineError::Fat(
            SdFatError::InvalidBootSector
            | SdFatError::BadCluster(_)
            | SdFatError::ClusterChainTooLong,
        ) => AssetReadError::BadHeader,
        _ => AssetReadError::Unavailable,
    }
}

fn map_pack_error(err: mountain_snow::pack::PackError) -> AssetReadError {
    match err {
        mountain_snow::pack::PackError::BadMagic => AssetReadError::BadMagic,
        mountain_snow::pack::PackError::BadCrc => AssetReadError::BadChecksum,
        mountain_snow::pack::PackError::BadFileLen
        | mountain_snow::pack::PackError::TruncatedPayload => AssetReadError::BadSize,
        mountain_snow::pack::PackError::BadReserved
        | mountain_snow::pack::PackError::BadGeometry
        | mountain_snow::pack::PackError::BadLengthField
        | mountain_snow::pack::PackError::BadRow => AssetReadError::BadHeader,
    }
}

fn map_compose_error(err: mountain_snow::composer::ComposerError) -> AssetReadError {
    match err {
        // The percent was validated before powering the card; the row
        // scratch planes are fixed-size, so neither arm is reachable.
        mountain_snow::composer::ComposerError::InvalidPercent => AssetReadError::BadHeader,
        mountain_snow::composer::ComposerError::RowLength
        | mountain_snow::composer::ComposerError::PackedLength => AssetReadError::BadSize,
    }
}
