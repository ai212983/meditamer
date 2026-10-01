#![deny(unsafe_code)]

//! Segmented SD read for the analog clock source-map pack.
//!
//! Serves one queued [`AssetReadRequest`] by streaming `/assets/CLOCK/MAPS.BIN`
//! into one nine-part generic runtime bundle in strict PSRAM. The FAT
//! stream observer routes sectors into their canonical map regions, so
//! no 2.5 MB contiguous allocation or FAT engine change is needed.
//!
//! Scheduling contract (see `clock_assets`): never interleave with an active
//! upload session (respond [`AssetReadError::Busy`] instead, which keeps the
//! upload session and engine valid), never call the disabled service-upload
//! handler, and never cancel an in-flight SD/DMA read with an external
//! timeout. Transport init owns its cooperative deadline; the read itself is
//! bounded by the service/transport timeouts. On transport failure the card
//! probe and shared engine are invalidated; the normal idle power-off path is
//! untouched.

use embassy_time::{Duration, Instant};
use phase_arena::{PartRequest, PartShape, RuntimeBundle};

use sdcard::fat::{
    FatEngine, FatEngineError, FatIoAction, FatPayloadId, FatRequest, FatResult, FatStageLabel,
    SdFatError,
};
use sdcard::service::{FatBuffers, FatObserver};
use sdcard::transport::TransportError;

use super::super::super::psram::{acquire_runtime_bundle, RuntimeBundleAcquireError};
use super::super::super::types::{SdPowerRequest, SdProbeDriver};
use super::super::ambient_assets::{self as ambient, AmbientReadCompletion, AmbientReadRequest};
use super::super::clock_assets::{
    asset_path_field, complete_asset_read, AssetBuffers, AssetReadCompletion, AssetReadError,
    AssetReadRequest, FILE_LEN,
};
use super::engine_driver::{run_fat_request, ProductObserver};
use super::request_sd_power;

// Cooperative init deadline shared with normal requests: `SdCardProbe`
// checks it only between completed DMA transfers, so expiry never cancels an
// in-flight DMA.
const ASSET_INIT_DEADLINE_MS: u64 = 8_000;

/// Serve one queued asset read to completion and publish the owned result.
/// Never blocks: the result channel `try_send` cannot stall the SD owner.
pub(super) async fn serve_asset_read(
    request: AssetReadRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
) {
    let result = run_asset_read(sd_probe, powered, upload_active, fat_engine).await;
    match &result {
        Ok(_) => console::println!("sdtask: clock_assets_read_ok id={}", request.id),
        Err(err) => console::println!(
            "sdtask: clock_assets_read_error id={} err={:?} retryable={}",
            request.id,
            err,
            err.is_retryable()
        ),
    }
    complete_asset_read(AssetReadCompletion {
        id: request.id,
        generation: request.generation,
        result,
    });
}

async fn run_asset_read(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
) -> Result<AssetBuffers, AssetReadError> {
    // An active upload owns the engine/session; report busy rather than
    // interleaving a read that would invalidate it.
    if upload_active {
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
            if !crate::firmware::update::transport_quiet() {
                console::println!("sdtask: clock_assets_init_error err={:?}", err);
            }
            return Err(AssetReadError::Unavailable);
        }
    }

    let requests: [PartRequest; 9] = core::array::from_fn(|i| PartRequest {
        bytes: ::clock_assets::MAP_LENGTHS[i],
        align: 1,
        shape: PartShape::Contiguous,
    });
    let mut bundle = match acquire_runtime_bundle(&requests) {
        Ok(bundle) => bundle,
        Err(RuntimeBundleAcquireError::AllocationFailed(_)) => {
            return Err(AssetReadError::OutOfMemory)
        }
        Err(RuntimeBundleAcquireError::AllocatorNotReady) => {
            return Err(AssetReadError::Unavailable)
        }
        Err(RuntimeBundleAcquireError::InvalidRequest(_)) => return Err(AssetReadError::BadSize),
    };
    let (path, path_len) = asset_path_field();
    let mut output = [];
    let mut observer = ClockStreamObserver::new(&mut bundle);
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
    let header = observer.header;
    let stream_invalid = observer.invalid || observer.seen != FILE_LEN;
    drop(observer);
    match read_result {
        FatResult::Streamed { bytes } if bytes as usize == FILE_LEN => {}
        FatResult::Streamed { bytes } => {
            console::println!(
                "sdtask: clock_assets_truncated bytes={} want={}",
                bytes,
                FILE_LEN
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

    ::clock_assets::decode_parts(
        &header,
        core::array::from_fn(|i| bundle.part(i).unwrap_or(&[])),
    )
    .map_err(super::super::clock_assets::map_decode_error)?;
    Ok(AssetBuffers::new(bundle))
}

struct ClockStreamObserver<'a> {
    bundle: &'a mut RuntimeBundle<esp_alloc::ExternalMemory>,
    header: [u8; ::clock_assets::HEADER_LEN],
    seen: usize,
    invalid: bool,
    product: ProductObserver,
}

impl<'a> ClockStreamObserver<'a> {
    fn new(bundle: &'a mut RuntimeBundle<esp_alloc::ExternalMemory>) -> Self {
        Self {
            bundle,
            header: [0; ::clock_assets::HEADER_LEN],
            seen: 0,
            invalid: false,
            product: ProductObserver,
        }
    }
}

impl FatObserver for ClockStreamObserver<'_> {
    fn on_stream_chunk(&mut self, offset: u32, bytes: &[u8]) {
        let offset = offset as usize;
        let end = offset.saturating_add(bytes.len());
        if offset != self.seen || end > FILE_LEN {
            self.invalid = true;
        }
        self.seen = end;
        if offset
            .checked_add(bytes.len())
            .is_none_or(|chunk_end| chunk_end > FILE_LEN)
        {
            self.invalid = true;
            return;
        }
        let chunk_end = offset + bytes.len();
        if offset < ::clock_assets::HEADER_LEN {
            let take = (::clock_assets::HEADER_LEN - offset).min(bytes.len());
            self.header[offset..offset + take].copy_from_slice(&bytes[..take]);
        }
        for (index, (map_offset, map_len)) in ::clock_assets::MAP_OFFSETS
            .iter()
            .zip(::clock_assets::MAP_LENGTHS)
            .enumerate()
        {
            let region_start = ::clock_assets::HEADER_LEN + map_offset;
            let lo = offset.max(region_start);
            let hi = chunk_end.min(region_start + map_len);
            if lo < hi
                && self
                    .bundle
                    .write_at(index, lo - region_start, &bytes[lo - offset..hi - offset])
                    .is_err()
            {
                self.invalid = true;
            }
        }
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
        FatEngineError::Fat(SdFatError::BufferTooSmall { .. }) => AssetReadError::BadSize,
        _ => AssetReadError::Unavailable,
    }
}

/// Serve one queued ambient sky/sun pack read to completion and publish the
/// owned result. Same scheduling contract as [`serve_asset_read`]: never
/// interleave with an active upload session, never cancel an in-flight
/// SD/DMA read with an external timeout.
pub(super) async fn serve_ambient_asset_read(
    request: AmbientReadRequest,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
) {
    let (result, len) = run_ambient_asset_read(sd_probe, powered, upload_active, fat_engine).await;
    match &result {
        Ok(_) => console::println!(
            "sdtask: ambient_assets_read_ok id={} bytes={}",
            request.id,
            len
        ),
        Err(err) => console::println!(
            "sdtask: ambient_assets_read_error id={} err={:?} retryable={}",
            request.id,
            err,
            err.is_retryable()
        ),
    }
    ambient::complete_asset_read(AmbientReadCompletion {
        id: request.id,
        generation: request.generation,
        result,
        len,
    });
}

async fn run_ambient_asset_read(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
) -> (Result<ambient::AssetBuffer, AssetReadError>, usize) {
    if let Err(err) = ensure_ambient_transport(sd_probe, powered, upload_active, fat_engine).await {
        return (Err(err), 0);
    }
    let (path, path_len) = ambient::asset_path_field();
    let stat_len = match stat_ambient_len(path, path_len, sd_probe, fat_engine).await {
        Ok(len) => len,
        Err(err) => return (Err(err), 0),
    };
    let header = match read_ambient_header(path, path_len, sd_probe, fat_engine).await {
        Ok(header) => header,
        Err(err) => return (Err(err), 0),
    };
    let file_len = match resolve_ambient_file_len(&header, stat_len) {
        Ok(len) => len,
        Err(err) => return (Err(err), 0),
    };
    let bundle = match acquire_ambient_bundle(file_len) {
        Ok(bundle) => bundle,
        Err(err) => return (Err(err), 0),
    };
    match stream_ambient_bundle(
        bundle, path, path_len, header, file_len, sd_probe, fat_engine,
    )
    .await
    {
        Ok(bundle) => (Ok(bundle), file_len),
        Err(err) => (Err(err), 0),
    }
}

async fn ensure_ambient_transport(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_active: bool,
    fat_engine: &mut FatEngine,
) -> Result<(), AssetReadError> {
    // An active upload owns the engine/session; report busy rather than
    // interleaving a read that would invalidate it.
    if upload_active {
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
            if !crate::firmware::update::transport_quiet() {
                console::println!("sdtask: ambient_assets_init_error err={:?}", err);
            }
            return Err(AssetReadError::Unavailable);
        }
    }
    Ok(())
}

async fn stat_ambient_len(
    path: [u8; sdcard::SD_PATH_MAX],
    path_len: u8,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Result<usize, AssetReadError> {
    let mut stat_output = [];
    match run_fat_request(
        FatRequest::Stat { path, path_len },
        sd_probe,
        fat_engine,
        &[],
        &mut stat_output,
    )
    .await
    {
        FatResult::Stat(entry) => Ok(entry.size as usize),
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

async fn read_ambient_header(
    path: [u8; sdcard::SD_PATH_MAX],
    path_len: u8,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Result<[u8; ambient::HEADER_LEN], AssetReadError> {
    let mut header = [0u8; ambient::HEADER_LEN];
    let header_len = ambient::HEADER_LEN as u32;
    match run_fat_request(
        FatRequest::ReadRange {
            path,
            path_len,
            offset: 0,
            len: header_len,
            output: FatPayloadId::Primary,
            output_capacity: header_len,
        },
        sd_probe,
        fat_engine,
        &[],
        &mut header,
    )
    .await
    {
        FatResult::Read { bytes } if bytes as usize == ambient::HEADER_LEN => Ok(header),
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

fn resolve_ambient_file_len(
    header: &[u8; ambient::HEADER_LEN],
    stat_len: usize,
) -> Result<usize, AssetReadError> {
    let file_len = match ::ambient_assets::file_len_from_header(header) {
        Ok(len) => len,
        Err(err) => return Err(ambient::map_decode_error(err)),
    };
    if stat_len != file_len {
        return Err(AssetReadError::BadSize);
    }
    Ok(file_len)
}

fn acquire_ambient_bundle(file_len: usize) -> Result<ambient::AssetBuffer, AssetReadError> {
    match acquire_runtime_bundle(&[PartRequest {
        bytes: file_len,
        align: 1,
        shape: PartShape::Contiguous,
    }]) {
        Ok(bundle) => Ok(bundle),
        Err(RuntimeBundleAcquireError::AllocationFailed(_)) => Err(AssetReadError::OutOfMemory),
        Err(RuntimeBundleAcquireError::AllocatorNotReady) => Err(AssetReadError::Unavailable),
        Err(RuntimeBundleAcquireError::InvalidRequest(_)) => Err(AssetReadError::BadSize),
    }
}

async fn stream_ambient_bundle(
    mut bundle: ambient::AssetBuffer,
    path: [u8; sdcard::SD_PATH_MAX],
    path_len: u8,
    header: [u8; ambient::HEADER_LEN],
    file_len: usize,
    sd_probe: &mut SdProbeDriver,
    fat_engine: &mut FatEngine,
) -> Result<ambient::AssetBuffer, AssetReadError> {
    let mut output = [];
    let mut observer = AmbientStreamObserver::new(&mut bundle);
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
    let seen = observer.seen;
    let streamed_header = observer.header;
    let stream_invalid = observer.invalid || seen != file_len;
    drop(observer);
    match read_result {
        FatResult::Streamed { bytes } if bytes as usize == file_len => {}
        FatResult::Streamed { bytes } => {
            console::println!(
                "sdtask: ambient_assets_truncated bytes={} want={}",
                bytes,
                file_len
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
    if streamed_header != header {
        return Err(AssetReadError::BadSize);
    }
    match bundle.part(0) {
        Some(part) => {
            if let Err(err) = ambient::validate_assets(part) {
                return Err(err);
            }
        }
        None => return Err(AssetReadError::BadSize),
    }
    Ok(bundle)
}

struct AmbientStreamObserver<'a> {
    bundle: &'a mut ambient::AssetBuffer,
    header: [u8; ambient::HEADER_LEN],
    seen: usize,
    invalid: bool,
    product: ProductObserver,
}

impl<'a> AmbientStreamObserver<'a> {
    fn new(bundle: &'a mut ambient::AssetBuffer) -> Self {
        Self {
            bundle,
            header: [0; ambient::HEADER_LEN],
            seen: 0,
            invalid: false,
            product: ProductObserver,
        }
    }
}

impl FatObserver for AmbientStreamObserver<'_> {
    fn on_stream_chunk(&mut self, offset: u32, bytes: &[u8]) {
        let offset = offset as usize;
        let end = offset.saturating_add(bytes.len());
        let Some(dest) = self.bundle.part_mut(0) else {
            self.invalid = true;
            return;
        };
        if offset != self.seen || end > dest.len() {
            self.invalid = true;
            return;
        }
        dest[offset..end].copy_from_slice(bytes);
        self.seen = end;
        if offset < ambient::HEADER_LEN {
            let take = (ambient::HEADER_LEN - offset).min(bytes.len());
            self.header[offset..offset + take].copy_from_slice(&bytes[..take]);
        }
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
