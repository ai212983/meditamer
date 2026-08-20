use super::dispatch::process_request;
use super::logging::publish_result;
#[cfg(feature = "asset-upload-http")]
use super::logging::{publish_upload_result, publish_wifi_config_response};
use super::power::{duration_ms_since, failure_backoff_ms, request_sd_power};
#[cfg(not(feature = "asset-upload-http"))]
use super::receive::receive_core_request;
#[cfg(feature = "asset-upload-http")]
use super::upload::process_upload_request;
#[cfg(feature = "asset-upload-http")]
use super::upload_ready::{
    abort_active_upload_session, disabled_upload_result, disabled_wifi_config_response,
    ensure_upload_storage_ready, wifi_config_error_response,
};
#[cfg(feature = "asset-upload-http")]
use super::wifi_config::process_wifi_config_request;
use super::SD_BOOT_POWER_OFF_GRACE_MS;
#[cfg(feature = "asset-upload-http")]
use super::SD_IDLE_POWER_OFF_MS;
#[cfg(feature = "asset-upload-http")]
use super::SD_UPLOAD_SESSION_IDLE_ABORT_MS;

#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select3, Either3};
#[cfg(feature = "asset-upload-http")]
use embassy_time::with_timeout;
use embassy_time::{Duration, Instant, Timer};
use sdcard::fat::FatEngine;
use sdcard::runtime as sd_ops;

#[cfg(feature = "asset-upload-http")]
use super::super::super::config::WIFI_CONFIG_REQUESTS;
#[cfg(feature = "asset-upload-http")]
use super::super::super::config::{SD_REQUESTS, SD_UPLOAD_REQUESTS};
#[cfg(feature = "asset-upload-http")]
use super::super::super::observability;
#[cfg(feature = "asset-upload-http")]
use super::super::super::service_mode;
use super::super::super::types::{SdPowerRequest, SdProbeDriver, SdRequest};
#[cfg(feature = "asset-upload-http")]
use super::super::super::types::{SdUploadRequest, SdUploadResult, SdUploadResultCode};
use super::upload::SdUploadSession;

struct SdTaskRuntime {
    sd_probe: SdProbeDriver,
    boot_started_at: Instant,
    powered: bool,
    upload_mounted: bool,
    upload_session: Option<SdUploadSession>,
    // Persistent FAT interpreter state has no ISR/DMA ownership. Keep it in
    // external PSRAM so dynamic Wi-Fi RX buffers retain the internal reserve.
    // SdCardProbe owns the internal DMA bounce sector used for all reads and
    // writes. See docs/reference/dram/dram-budget.md.
    fat_engine: crate::firmware::psram::ExternalValue<FatEngine>,
    consecutive_failures: u8,
    backoff_until: Option<Instant>,
}

impl SdTaskRuntime {
    async fn initialize(sd_probe: SdProbeDriver) -> Self {
        let fat_engine = FatEngine::new();
        // The value is materialized before it is copied into PSRAM. Capture
        // that transient frame so the runtime CPU0 floor covers construction.
        crate::firmware::observability::record_stack_headroom();
        let mut runtime = Self {
            sd_probe,
            boot_started_at: Instant::now(),
            powered: false,
            upload_mounted: false,
            upload_session: None,
            fat_engine: match crate::firmware::psram::ExternalValue::try_new(fat_engine) {
                Ok(engine) => engine,
                Err(_) => {
                    console::println!("sdtask: external FAT engine allocation failed");
                    crate::firmware::reset_pending_update_or_halt();
                }
            },
            consecutive_failures: 0,
            backoff_until: None,
        };
        let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
        (runtime.consecutive_failures, runtime.backoff_until) = super::runtime_startup::initialize(
            &mut runtime.sd_probe,
            &mut runtime.powered,
            &mut no_power,
            &mut runtime.fat_engine,
        )
        .await;
        runtime
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
            &mut self.sd_probe,
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
        match select3(
            WIFI_CONFIG_REQUESTS.receive(),
            SD_UPLOAD_REQUESTS.receive(),
            with_timeout(
                Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                SD_REQUESTS.receive(),
            ),
        )
        .await
        {
            Either3::First(request) => {
                self.process_wifi_config_request(request).await;
            }
            Either3::Second(request) => {
                self.process_upload_request(request).await;
            }
            Either3::Third(Ok(request)) => self.process_core_request(request).await,
            Either3::Third(Err(_)) => self.handle_idle().await,
        }
    }

    #[cfg(feature = "asset-upload-http")]
    async fn run_unpowered_cycle(&mut self) {
        match select3(
            WIFI_CONFIG_REQUESTS.receive(),
            SD_UPLOAD_REQUESTS.receive(),
            SD_REQUESTS.receive(),
        )
        .await
        {
            Either3::First(request) => {
                self.process_wifi_config_request(request).await;
            }
            Either3::Second(request) => {
                self.process_upload_request(request).await;
            }
            Either3::Third(request) => self.process_core_request(request).await,
        }
    }

    #[cfg(feature = "asset-upload-http")]
    pub(super) async fn process_wifi_config_request(
        &mut self,
        request: crate::firmware::types::WifiConfigRequest,
    ) {
        let request_id = request.request_id();
        if !service_mode::upload_transfers_enabled() {
            publish_wifi_config_response(disabled_wifi_config_response(request_id));
            return;
        }
        if let Err(code) = ensure_upload_storage_ready(
            &mut self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await
        {
            self.upload_session = None;
            self.upload_mounted = false;
            self.fat_engine.invalidate();
            publish_wifi_config_response(wifi_config_error_response(request_id, code));
            return;
        }
        let response = process_wifi_config_request(
            request,
            &self.upload_session,
            &mut self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await;
        publish_wifi_config_response(response);
    }

    #[cfg(feature = "asset-upload-http")]
    pub(super) async fn process_upload_request(&mut self, request: SdUploadRequest) {
        let request_id = request.id;
        if !service_mode::upload_transfers_enabled() {
            publish_upload_result(disabled_upload_result(request_id));
            return;
        }
        if let Err(code) = ensure_upload_storage_ready(
            &mut self.sd_probe,
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
            &mut self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.fat_engine,
        )
        .await;
        super::super::upload::set_sd_upload_session_active(self.upload_session.is_some());
        publish_upload_result(result);
    }

    #[cfg(not(feature = "asset-upload-http"))]
    async fn run_cycle(&mut self) {
        match receive_core_request(
            &mut self.sd_probe,
            &mut self.powered,
            &mut self.upload_mounted,
            &mut self.upload_session,
            &mut self.fat_engine,
        )
        .await
        {
            Some(request) => self.process_core_request(request).await,
            None => self.handle_idle().await,
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
        self.reset_storage_state();
    }

    async fn process_core_request(&mut self, request: SdRequest) {
        let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
        let result = process_request(
            request,
            &mut self.sd_probe,
            &mut self.powered,
            &mut no_power,
            &mut self.fat_engine,
        )
        .await;
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
        self.reset_storage_state();
    }

    fn reset_storage_state(&mut self) {
        self.powered = false;
        self.sd_probe.invalidate();
        self.fat_engine.invalidate();
        self.upload_mounted = false;
        self.upload_session = None;
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
pub(crate) async fn sd_task(sd_probe: SdProbeDriver) {
    let mut runtime = SdTaskRuntime::initialize(sd_probe).await;
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
