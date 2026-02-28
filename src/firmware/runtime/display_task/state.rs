use embassy_time::{Duration, Instant};

use super::super::super::{
    app_state::{
        publish_app_state_snapshot, AppStateApplyResult, AppStateCommand, AppStateEngine,
        AppStateSnapshot, BaseMode, DayBackground, OverlayMode,
    },
    config::DIAG_CONTROL_EVENTS,
    event_engine::{EngineTraceSample, EventEngine},
    storage::transfer_buffers,
    touch::{
        config::{TOUCH_CALIBRATION_WIZARD_ENABLED, TOUCH_INIT_RETRY_MS},
        tasks::try_touch_init_with_logs,
        wizard::TouchCalibrationWizard,
    },
    types::{DisplayContext, TimeSyncState},
};
use super::super::FaceDownToggleState;
#[cfg(feature = "graphics")]
use crate::firmware::assets::runtime::clear_runtime_asset_caches;

pub(super) struct DisplayLoopState {
    pub(super) update_count: u32,
    pub(super) last_uptime_seconds: u32,
    pub(super) time_sync: Option<TimeSyncState>,
    pub(super) battery_percent: Option<u8>,
    pub(super) app_state: AppStateEngine,
    pub(super) snapshot: AppStateSnapshot,
    pub(super) screen_initialized: bool,
    pub(super) pattern_nonce: u32,
    pub(super) first_visual_seed_pending: bool,
    pub(super) face_down_toggle: FaceDownToggleState,
    pub(super) imu_double_tap_ready: bool,
    pub(super) imu_retry_at: Instant,
    pub(super) event_engine: EventEngine,
    pub(super) last_engine_trace: EngineTraceSample,
    pub(super) last_detect_tap_src: u8,
    pub(super) last_detect_int1: u8,
    pub(super) trace_epoch: Instant,
    pub(super) tap_trace_next_sample_at: Instant,
    pub(super) tap_trace_aux_next_sample_at: Instant,
    pub(super) tap_trace_power_good: i16,
    pub(super) backlight_cycle_start: Option<Instant>,
    pub(super) backlight_level: u8,
    pub(super) touch_ready: bool,
    pub(super) touch_wizard: TouchCalibrationWizard,
    pub(super) touch_retry_at: Instant,
    pub(super) touch_next_sample_at: Instant,
    pub(super) touch_feedback_dirty: bool,
    pub(super) touch_feedback_next_flush_at: Instant,
    pub(super) touch_contact_active: bool,
    pub(super) touch_last_nonzero_at: Option<Instant>,
    pub(super) touch_irq_pending: u8,
    pub(super) touch_irq_burst_until: Instant,
    pub(super) touch_idle_fallback_at: Instant,
    pub(super) touch_wizard_trace_capture_until_ms: u64,
}

impl DisplayLoopState {
    pub(super) async fn new(context: &mut DisplayContext) -> Self {
        let now = Instant::now();
        let persisted = context.app_state_store.load_state().unwrap_or_default();
        let mut app_state = AppStateEngine::from_persisted(persisted);
        let boot_result = app_state.apply(AppStateCommand::BootComplete);
        if let Some(control) = boot_result.diag_control() {
            DIAG_CONTROL_EVENTS.send(control).await;
        }
        if TOUCH_CALIBRATION_WIZARD_ENABLED {
            let wizard_result = app_state.apply(AppStateCommand::SetBase(BaseMode::TouchWizard));
            if let Some(control) = wizard_result.diag_control() {
                DIAG_CONTROL_EVENTS.send(control).await;
            }
        }
        let snapshot = app_state.snapshot();
        publish_app_state_snapshot(snapshot);
        let touch_ready = try_touch_init_with_logs(&mut context.inkplate, "boot");
        let touch_wizard = TouchCalibrationWizard::new(
            matches!(snapshot.base, BaseMode::TouchWizard) && touch_ready,
        );
        let touch_retry_at = if touch_ready {
            now
        } else {
            now + Duration::from_millis(TOUCH_INIT_RETRY_MS)
        };

        Self {
            update_count: 0,
            last_uptime_seconds: 0,
            time_sync: None,
            battery_percent: None,
            app_state,
            snapshot,
            screen_initialized: false,
            pattern_nonce: 0,
            first_visual_seed_pending: true,
            face_down_toggle: FaceDownToggleState::new(),
            imu_double_tap_ready: false,
            imu_retry_at: now,
            event_engine: EventEngine::default(),
            last_engine_trace: EngineTraceSample::default(),
            last_detect_tap_src: 0,
            last_detect_int1: 0,
            trace_epoch: now,
            tap_trace_next_sample_at: now,
            tap_trace_aux_next_sample_at: now,
            tap_trace_power_good: -1,
            backlight_cycle_start: None,
            backlight_level: 0,
            touch_ready,
            touch_wizard,
            touch_retry_at,
            touch_next_sample_at: now,
            touch_feedback_dirty: false,
            touch_feedback_next_flush_at: now,
            touch_contact_active: false,
            touch_last_nonzero_at: None,
            touch_irq_pending: 0,
            touch_irq_burst_until: now,
            touch_idle_fallback_at: now,
            touch_wizard_trace_capture_until_ms: 0,
        }
    }

    pub(super) async fn apply_state_command(
        &mut self,
        context: &mut DisplayContext,
        command: AppStateCommand,
    ) -> AppStateApplyResult {
        let result = self.app_state.apply(command);
        if !result.changed() {
            return result;
        }

        self.snapshot = result.after;
        publish_app_state_snapshot(self.snapshot);
        if let Some(control) = result.diag_control() {
            DIAG_CONTROL_EVENTS.send(control).await;
        }

        if result.persist_required() {
            context.app_state_store.save_state(
                crate::firmware::app_state::PersistedAppState::from_snapshot(result.after),
            );
        }

        if result.services_changed() {
            if result.before.services.upload_enabled && !result.after.services.upload_enabled {
                transfer_buffers::release_upload_chunk_buffer().await;
            }
            if result.before.services.asset_reads_enabled
                && !result.after.services.asset_reads_enabled
            {
                transfer_buffers::release_asset_read_buffer().await;
                #[cfg(feature = "graphics")]
                clear_runtime_asset_caches().await;
            }
            if !result.before.services.upload_enabled && result.after.services.upload_enabled {
                let _ = context.inkplate.frontlight_off();
            }
        }

        result
    }

    pub(super) fn base_mode(&self) -> BaseMode {
        self.snapshot.base
    }

    pub(super) fn day_background(&self) -> DayBackground {
        self.snapshot.day_background
    }

    pub(super) fn overlay_mode(&self) -> OverlayMode {
        self.snapshot.overlay
    }

    pub(super) fn upload_enabled(&self) -> bool {
        self.snapshot.services.upload_enabled
    }

    pub(super) fn in_touch_wizard_mode(&self) -> bool {
        matches!(self.snapshot.base, BaseMode::TouchWizard)
    }
}
