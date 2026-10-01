use super::dispatch::process_request;
use super::logging::publish_result;
#[cfg(feature = "asset-upload-http")]
use super::logging::publish_upload_result;
use super::power::{duration_ms_since, failure_backoff_ms, request_sd_power};
#[cfg(not(feature = "asset-upload-http"))]
use super::receive::receive_core_request;
#[cfg(feature = "asset-upload-http")]
use super::upload::process_upload_request;
#[cfg(feature = "asset-upload-http")]
use super::upload::{no_session_abort_fence_result, route_upload_request, UploadRequestRoute};
#[cfg(feature = "asset-upload-http")]
use super::upload_ready::{
    abort_active_upload_session, disabled_upload_result, ensure_upload_storage_ready,
};
use super::SD_BOOT_POWER_OFF_GRACE_MS;
#[cfg(feature = "asset-upload-http")]
use super::SD_IDLE_POWER_OFF_MS;
#[cfg(feature = "asset-upload-http")]
use super::SD_UPLOAD_SESSION_IDLE_ABORT_MS;

use core::future::Future;

#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select, Either};
#[cfg(feature = "asset-upload-http")]
use embassy_time::with_timeout;
use embassy_time::{Duration, Instant, Timer};
use sdcard::fat::FatEngine;
use sdcard::runtime as sd_ops;

#[cfg(feature = "asset-upload-http")]
use super::super::super::config::{SD_REQUESTS, SD_UPLOAD_REQUESTS};
#[cfg(feature = "asset-upload-http")]
use super::super::super::observability;
use super::super::super::psram::{BufferAllocError, ExternalValue};
#[cfg(feature = "asset-upload-http")]
use super::super::super::service_mode;
use super::super::super::types::{SdPowerRequest, SdProbeDriver, SdRequest};
#[cfg(feature = "asset-upload-http")]
use super::super::super::types::{SdUploadRequest, SdUploadResult, SdUploadResultCode};
use super::upload::SdUploadSession;

#[cfg(feature = "sd-runner-allocation-fixture")]
mod allocation_fixture;

struct SdTaskRuntime<'a> {
    sd_probe: &'a mut SdProbeDriver,
    boot_started_at: Instant,
    powered: bool,
    upload_mounted: bool,
    upload_session: Option<SdUploadSession>,
    // Cached mountain validation metadata for the current upload
    // generation. Owned here so it lives inside the existing
    // `ExternalValue`-boxed runner future — never a static — and is
    // threaded only into the mountain handler.
    mountain_session: Option<super::mountain_read::MountainSession>,
    // Persistent FAT interpreter state has no ISR/DMA ownership. Keep it in
    // external PSRAM so dynamic Wi-Fi RX buffers retain the internal reserve.
    // The outer task retains the probe, including its SPI/DMA handles and
    // protocol staging frame, in internal RAM. See docs/references/memory/meditamer-inkplate/budget.md.
    fat_engine: crate::firmware::psram::ExternalValue<FatEngine>,
    consecutive_failures: u8,
    backoff_until: Option<Instant>,
}

impl<'a> SdTaskRuntime<'a> {
    fn new(sd_probe: &'a mut SdProbeDriver, fat_engine: ExternalValue<FatEngine>) -> Self {
        Self {
            sd_probe,
            boot_started_at: Instant::now(),
            powered: false,
            upload_mounted: false,
            upload_session: None,
            mountain_session: None,
            fat_engine,
            consecutive_failures: 0,
            backoff_until: None,
        }
    }

    async fn initialize(&mut self) {
        let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
        (self.consecutive_failures, self.backoff_until) = super::runtime_startup::initialize(
            self.sd_probe,
            &mut self.powered,
            &mut no_power,
            &mut self.fat_engine,
        )
        .await;
    }

    async fn wait_for_backoff(&mut self) {
        let Some(until) = self.backoff_until.take() else {
            return;
        };
        let now = Instant::now();
        if now < until {
            Timer::after(until.saturating_duration_since(now)).await;
        }
    }

    #[cfg(feature = "asset-upload-http")]
    async fn abort_stale_upload_session(&mut self) -> bool {
        let Some(reason) = self.pending_upload_abort() else {
            return false;
        };
        match reason {
            UploadAbortReason::ModeOff => observability::record_sd_upload_session_mode_off_abort(),
            UploadAbortReason::Idle { idle_ms } => {
                observability::record_sd_upload_session_timeout_abort();
                console::println!(
                    "sdtask: upload_session_idle_abort idle_ms={} threshold_ms={}",
                    idle_ms,
                    SD_UPLOAD_SESSION_IDLE_ABORT_MS
                );
            }
        }
        let result = abort_active_upload_session(
            &mut self.upload_session,
            self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await;
        super::super::upload::set_sd_upload_session_active(self.upload_session.is_some());
        if !result.ok {
            console::println!(
                "sdtask: autonomous_upload_abort_failed code={:?}",
                result.code
            );
        }
        true
    }

    #[cfg(feature = "asset-upload-http")]
    fn pending_upload_abort(&self) -> Option<UploadAbortReason> {
        self.upload_session.as_ref()?;
        if !service_mode::upload_transfers_enabled() {
            return Some(UploadAbortReason::ModeOff);
        }
        let last_activity_at = super::upload::active_session_last_activity(&self.upload_session)?;
        let idle_ms = duration_ms_since(last_activity_at);
        (idle_ms >= SD_UPLOAD_SESSION_IDLE_ABORT_MS).then_some(UploadAbortReason::Idle { idle_ms })
    }

    #[cfg(feature = "asset-upload-http")]
    async fn run_cycle(&mut self) {
        if self.powered {
            self.run_powered_cycle().await;
        } else {
            self.run_unpowered_cycle().await;
        }
    }

    #[cfg(feature = "asset-upload-http")]
    async fn run_powered_cycle(&mut self) {
        // Channel-only intake: the asset receive joins the same select as
        // upload/core plus the idle deadline, so a request queued while the
        // owner is unpowered (or busy elsewhere) still wakes it. The match
        // classifies owned input first; handlers are awaited only after the
        // select resolves — never raced inside it, which would cancel an
        // in-flight SD/DMA transfer. Upload admission, abort, and priority
        // over core traffic are unchanged.
        match select(
            super::super::clock_assets::receive_asset_request(),
            select(
                super::super::ambient_assets::receive_asset_request(),
                select(
                    super::super::mountain_assets::receive_asset_request(),
                    select(
                        SD_UPLOAD_REQUESTS.receive(),
                        with_timeout(
                            Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                            SD_REQUESTS.receive(),
                        ),
                    ),
                ),
            ),
        )
        .await
        {
            Either::First(asset) => {
                self.serve_asset_request(asset).await;
            }
            Either::Second(Either::First(asset)) => {
                self.serve_ambient_asset_request(asset).await;
            }
            Either::Second(Either::Second(Either::First(asset))) => {
                self.serve_mountain_asset_request(asset).await;
            }
            Either::Second(Either::Second(Either::Second(Either::First(request)))) => {
                self.process_upload_request(request).await;
            }
            Either::Second(Either::Second(Either::Second(Either::Second(Ok(request))))) => {
                self.process_core_request(request).await
            }
            Either::Second(Either::Second(Either::Second(Either::Second(Err(_))))) => {
                self.handle_idle().await
            }
        }
    }

    #[cfg(feature = "asset-upload-http")]
    async fn run_unpowered_cycle(&mut self) {
        // Same channel-only intake without the idle deadline: any of the
        // three queued inputs wakes the unpowered owner.
        match select(
            super::super::clock_assets::receive_asset_request(),
            select(
                super::super::ambient_assets::receive_asset_request(),
                select(
                    super::super::mountain_assets::receive_asset_request(),
                    select(SD_UPLOAD_REQUESTS.receive(), SD_REQUESTS.receive()),
                ),
            ),
        )
        .await
        {
            Either::First(asset) => {
                self.serve_asset_request(asset).await;
            }
            Either::Second(Either::First(asset)) => {
                self.serve_ambient_asset_request(asset).await;
            }
            Either::Second(Either::Second(Either::First(asset))) => {
                self.serve_mountain_asset_request(asset).await;
            }
            Either::Second(Either::Second(Either::Second(Either::First(request)))) => {
                self.process_upload_request(request).await;
            }
            Either::Second(Either::Second(Either::Second(Either::Second(request)))) => {
                self.process_core_request(request).await
            }
        }
    }

    #[cfg(feature = "asset-upload-http")]
    pub(super) async fn process_upload_request(&mut self, request: SdUploadRequest) {
        // The supervisor Abort fence runs before transfer admission and
        // storage readiness. The single SD owner FIFO already serialized
        // earlier queued work, so this only passes the mode/readiness gates,
        // never another command.
        if let UploadRequestRoute::AbortFence { session_active } =
            route_upload_request(&request.command, self.upload_session.is_some())
        {
            self.serve_abort_fence(request, session_active).await;
            return;
        }
        // Upload buffers may coexist with this task; the optional Mountain
        // cache has no lease during upload mode.
        self.release_mountain_session("upload");
        let request_id = request.id;
        if !service_mode::upload_transfers_enabled() {
            publish_upload_result(disabled_upload_result(request_id));
            return;
        }
        if let Err(code) = ensure_upload_storage_ready(
            self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await
        {
            self.upload_session = None;
            self.upload_mounted = false;
            self.fat_engine.invalidate();
            publish_upload_result(upload_storage_error_result(request_id, code));
            return;
        }
        let result = process_upload_request(
            request,
            &mut self.upload_session,
            self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await;
        super::super::upload::set_sd_upload_session_active(self.upload_session.is_some());
        publish_upload_result(result);
    }

    #[cfg(feature = "asset-upload-http")]
    async fn serve_abort_fence(&mut self, request: SdUploadRequest, session_active: bool) {
        if !session_active {
            // No session owns temp state. This arm holds no probe or engine
            // reference, so it cannot power, mount, or initialize the card.
            super::super::upload::set_sd_upload_session_active(false);
            publish_upload_result(no_session_abort_fence_result(request.id));
            return;
        }
        // An active session performs real FAT cleanup below, which needs the
        // same Mountain-cache-free upload-mode memory as ordinary transfers.
        self.release_mountain_session("upload");
        let result = process_upload_request(
            request,
            &mut self.upload_session,
            self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await;
        super::super::upload::set_sd_upload_session_active(self.upload_session.is_some());
        publish_upload_result(result);
    }

    /// Serves one queued clock-asset whole-file read. Shared by both intake
    /// paths; an active upload session yields `Busy` instead of interleaving.
    async fn serve_asset_request(&mut self, request: super::super::clock_assets::AssetReadRequest) {
        // Release the optional Mountain cache before admitting the large
        // clock asset bundle, including on a rapid screen transition.
        self.release_mountain_session("clock_entry");
        let upload_active = self.upload_session.is_some();
        super::asset_read::serve_asset_read(
            request,
            self.sd_probe,
            &mut self.powered,
            upload_active,
            &mut self.fat_engine,
        )
        .await;
    }

    /// Serves one queued ambient sky/sun pack read. Same contract as
    /// [`Self::serve_asset_request`]: an active upload session yields
    /// `Busy` instead of interleaving.
    async fn serve_ambient_asset_request(
        &mut self,
        request: super::super::ambient_assets::AmbientReadRequest,
    ) {
        let upload_active = self.upload_session.is_some();
        super::asset_read::serve_ambient_asset_read(
            request,
            self.sd_probe,
            &mut self.powered,
            upload_active,
            &mut self.fat_engine,
        )
        .await;
    }

    async fn serve_mountain_asset_request(
        &mut self,
        command: super::super::mountain_assets::MountainCommand,
    ) {
        match command {
            super::super::mountain_assets::MountainCommand::Render(request) => {
                super::mountain_read::serve_mountain_asset_read(
                    request,
                    self.sd_probe,
                    &mut self.powered,
                    self.upload_session.is_some(),
                    &mut self.fat_engine,
                    &mut self.mountain_session,
                )
                .await;
            }
            super::super::mountain_assets::MountainCommand::ReleaseResident => {}
        }
        if !super::super::mountain_assets::resident_wanted() {
            self.release_mountain_session("screen_exit");
        }
    }

    #[cfg(not(feature = "asset-upload-http"))]
    async fn run_cycle(&mut self) {
        // Minimal-path intake: the asset receive already joined the
        // channel-only select inside `receive_core_request`, so no poll is
        // needed here. Classify the owned decision first, then await the
        // matching handler; upload handling inside intake is untouched.
        match receive_core_request(
            self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.upload_session,
            &mut self.mountain_session,
            &mut self.fat_engine,
        )
        .await
        {
            super::receive::CoreIntake::Asset(asset) => self.serve_asset_request(asset).await,
            super::receive::CoreIntake::AmbientAsset(asset) => {
                self.serve_ambient_asset_request(asset).await
            }
            super::receive::CoreIntake::MountainAsset(asset) => {
                self.serve_mountain_asset_request(asset).await
            }
            super::receive::CoreIntake::Core(request) => self.process_core_request(request).await,
            super::receive::CoreIntake::Idle => self.handle_idle().await,
        }
    }

    async fn handle_idle(&mut self) {
        #[cfg(feature = "asset-upload-http")]
        if self.upload_session.is_some() {
            // Keep SD online during an active upload session; stale sessions are cleaned up
            // by the idle-abort/mode-off check at the top of the loop.
            return;
        }
        // The boot probe can finish while the display task is still in its
        // initial e-paper refresh and unable to service the shared I2C
        // expander. Keep the card powered until the display has announced
        // runtime readiness; a fixed grace period alone raced slow full
        // refreshes and produced a spurious power-off timeout.
        if self.powered
            && (!crate::firmware::scheduling::runtime_ready()
                || duration_ms_since(self.boot_started_at) < SD_BOOT_POWER_OFF_GRACE_MS as u32)
        {
            return;
        }
        if self.powered && !request_sd_power(SdPowerRequest::Off).await {
            console::println!("sdtask: idle_power_off_failed");
        }
        // Power cycling the same card does not change its bytes. Preserve the
        // generation-bound Mountain validation session so the next percent
        // render does not repeat the full-file CRC and histogram pass.
        self.reset_storage_state(false);
    }

    async fn process_core_request(&mut self, request: SdRequest) {
        // A successful core mutation of the exact mountain pack path
        // replaces the file outside the upload flow; bump the generation so
        // the cached session and the retained overlay invalidate. Failed
        // operations and unrelated paths never bump.
        let mountain_mutation =
            super::super::mountain_assets::core_mutation_invalidates_generation(&request.command);
        let sky_mutation =
            super::super::ambient_assets::core_mutation_invalidates_generation(&request.command);
        let clock_mutation =
            super::super::clock_assets::core_mutation_invalidates_generation(&request.command);
        let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
        let result = process_request(
            request,
            self.sd_probe,
            &mut self.powered,
            &mut no_power,
            &mut self.fat_engine,
        )
        .await;
        if result.ok && mountain_mutation {
            super::super::mountain_assets::note_committed_upload();
            self.release_mountain_session("core_mutation");
        }
        if result.ok && sky_mutation {
            super::super::ambient_assets::note_committed_upload();
        }
        if result.ok && clock_mutation {
            super::super::clock_assets::note_committed_upload();
        }
        publish_result(result);
        if result.ok || !result.recover_bus {
            self.consecutive_failures = 0;
            self.backoff_until = None;
            return;
        }

        self.consecutive_failures = self.consecutive_failures.saturating_add(1).min(8);
        let backoff_ms = failure_backoff_ms(self.consecutive_failures);
        self.backoff_until = Some(Instant::now() + Duration::from_millis(backoff_ms));
        if self.powered && !request_sd_power(SdPowerRequest::Off).await {
            console::println!("sdtask: fail_power_off_failed");
        }
        self.reset_storage_state(true);
    }

    fn reset_storage_state(&mut self, clear_mountain_session: bool) {
        self.powered = false;
        self.sd_probe.invalidate();
        self.fat_engine.invalidate();
        self.upload_mounted = false;
        self.upload_session = None;
        if clear_mountain_session {
            // A bus/transport recovery cannot prove source continuity. Drop
            // the cached header/histogram so the next request revalidates.
            self.release_mountain_session("storage_recovery");
        }
    }

    fn release_mountain_session(&mut self, reason: &str) {
        if let Some(session) = self.mountain_session.take() {
            let bytes = session.resident_bytes();
            if bytes != 0 {
                console::println!(
                    "MOUNTAIN_CACHE status=released reason={} bytes={}",
                    reason,
                    bytes
                );
            }
            drop(session);
        }
    }
}

#[cfg(feature = "asset-upload-http")]
enum UploadAbortReason {
    ModeOff,
    Idle { idle_ms: u32 },
}

#[cfg(feature = "asset-upload-http")]
fn upload_storage_error_result(request_id: u32, code: SdUploadResultCode) -> SdUploadResult {
    SdUploadResult {
        request_id,
        ok: false,
        code,
        bytes_written: 0,
        chunk_queue_wait_ms: 0,
        chunk_handler_ms: 0,
        chunk_post_handler_ms: 0,
        chunk_published_at_ms: 0,
        chunk_handler_done_at_ms: 0,
    }
}

#[embassy_executor::task]
pub async fn sd_task(mut sd_probe: SdProbeDriver) {
    // Keep the executor header and hardware anchor internal. Only the SD
    // protocol/dispatch future lives externally; IRQs wake this outer task.
    #[cfg(feature = "sd-runner-allocation-fixture")]
    allocation_fixture::verify(&mut sd_probe);
    let fat_engine = allocate_fat_engine().unwrap_or_else(|_| {
        console::println!("sdtask: external FAT engine allocation failed");
        crate::firmware::reset_pending_update_or_halt();
    });
    let mut runner = allocate_runner(&mut sd_probe, fat_engine).unwrap_or_else(|_| {
        console::println!("sdtask: external runner allocation failed");
        crate::firmware::reset_pending_update_or_halt();
    });
    crate::firmware::observability::record_stack_headroom();
    let runner = runner.pin_mut();
    runner.await;
}

#[inline(never)]
fn allocate_fat_engine() -> Result<ExternalValue<FatEngine>, BufferAllocError> {
    let engine = ExternalValue::try_new_with(FatEngine::new)?;
    // Sample the one-time allocation path separately from steady task polling.
    crate::firmware::observability::record_stack_headroom();
    Ok(engine)
}

#[inline(never)]
fn allocate_runner<'a>(
    sd_probe: &'a mut SdProbeDriver,
    fat_engine: ExternalValue<FatEngine>,
) -> Result<ExternalValue<impl Future<Output = ()> + 'a>, BufferAllocError> {
    let runner = ExternalValue::try_new_with(|| run_sd(sd_probe, fat_engine))?;
    // A pointer-sized return keeps this one-time path out of steady polling.
    crate::firmware::observability::record_stack_headroom();
    Ok(runner)
}

async fn run_sd(sd_probe: &mut SdProbeDriver, fat_engine: ExternalValue<FatEngine>) {
    #[cfg(feature = "sd-runner-allocation-fixture")]
    allocation_fixture::record_runner_poll();
    let mut runtime = SdTaskRuntime::new(sd_probe, fat_engine);
    runtime.initialize().await;
    #[cfg(feature = "asset-upload-http")]
    super::super::upload::set_sd_upload_session_active(false);

    #[cfg(feature = "asset-upload-http")]
    if crate::firmware::storage::transfer_buffers::lock_upload_chunk_buffer()
        .await
        .is_err()
    {
        console::println!("sdtask: upload_chunk_buffer_prewarm_failed");
    }

    loop {
        runtime.wait_for_backoff().await;

        #[cfg(feature = "asset-upload-http")]
        if runtime.abort_stale_upload_session().await {
            continue;
        }

        runtime.run_cycle().await;
    }
}
