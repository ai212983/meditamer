use embassy_time::Instant;

use super::super::{
    app_state::{
        publish_app_state_snapshot, AppStateApplyResult, AppStateCommand, AppStateEngine,
        AppStateSnapshot,
    },
    config::DIAG_CONTROL_EVENTS,
    input::gpio36::Gpio36Mode,
    psram::{BufferAllocError, ExternalValue},
    storage::transfer_buffers,
    touch::config::GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED,
    types::DisplayContext,
};

pub(super) struct DisplayLoopState {
    pub(super) app_state: AppStateEngine,
    pub(super) snapshot: AppStateSnapshot,
    pub(super) backlight_cycle_start: Option<Instant>,
    pub(super) backlight_level: u8,
    /// Runtime-only frontlight baseline the short-press flash fades back
    /// to. Off at boot, selected by the WAKE long-press calibration; never
    /// persisted.
    /// Hardware writes stay in the frontlight owner (display/core 0 owns
    /// the frontlight); the panel service-transition guard forbids driver
    /// writes here, so this stays a plain level.
    pub(super) frontlight_baseline: u8,
    /// Live, reversible calibration opened by WAKE long press. While set,
    /// `preview` is the steady hardware target but `frontlight_baseline`
    /// remains unchanged until the user chooses On.
    pub(super) frontlight_calibration:
        Option<super::frontlight_policy::FrontlightCalibrationSession>,
    /// Press armed from the classified WAKE `Pressed` edge. The short action
    /// fires on release and calibration opens at the hold deadline;
    /// `None` while no hold owns the outcome.
    pub(super) wake_press: Option<super::frontlight_policy::WakePress>,
    /// Bounded retry timestamp after a failed frontlight application.
    pub(super) backlight_retry_at_ms: Option<u64>,
    pub(super) touch_startup_settled: bool,
    pub(super) runtime_ready_announced: bool,
    pub(super) gpio36_mode: Gpio36Mode,
    pub(super) presentation: super::presentation::PresentationState,
    /// Committed owner, health and delivery tracking for Ambient Home's BME688
    /// consumer (typed observation subscriptions plan, Phase 4). Not
    /// part of `presentation`: it belongs to the subscription/delivery
    /// decision, not to what gets rendered.
    pub(super) environment_delivery: super::environment_delivery::EnvironmentDeliveryState,
    /// Persistent trace consumer, independent of navigation and panel repaint.
    #[cfg(feature = "cpu-load")]
    pub(super) cpu_delivery: super::cpu_delivery::CpuDeliveryState,
    pub(super) battery_delivery: super::battery_delivery::BatteryDeliveryState,
}

impl DisplayLoopState {
    pub(super) async fn new(
        context: &mut DisplayContext,
    ) -> Result<ExternalValue<Self>, BufferAllocError> {
        #[cfg(feature = "ui-initialization-fixture")]
        Self::verify_allocation_failure();
        let persisted = context.app_state_store.load_state().unwrap_or_default();
        let mut app_state = AppStateEngine::from_persisted(persisted);
        let boot_result = app_state.apply(AppStateCommand::BootComplete);
        let snapshot = app_state.snapshot();
        // Allocate before live LVGL resources exist. Only the external owner
        // and small boot inputs survive the control-channel await below.
        let state = Self::allocate(app_state, snapshot)?;
        if let Some(control) = boot_result.diag_control() {
            DIAG_CONTROL_EVENTS.send(control).await;
        }
        publish_app_state_snapshot(snapshot);
        Ok(state)
    }

    #[cfg(feature = "ui-initialization-fixture")]
    #[inline(never)]
    fn verify_allocation_failure() {
        let before = esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::External.into());
        let app_state = AppStateEngine::from_persisted(Default::default());
        let snapshot = app_state.snapshot();
        crate::firmware::psram::reject_next_external_allocation();
        assert!(matches!(
            Self::allocate(app_state, snapshot),
            Err(BufferAllocError::OutOfMemory)
        ));
        let after = esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::External.into());
        assert_eq!(before, after);
        assert!(!render::lvgl_adapter::is_initialized());
        console::println!("UI_INIT_FIXTURE stage=model_rejected outcome=passed external_before={} external_after={}", before, after);
    }

    /// Allocate the model's backing storage before constructing it. Hardware,
    /// interrupt state and await machinery remain outside this value.
    #[inline(never)]
    fn allocate(
        app_state: AppStateEngine,
        snapshot: AppStateSnapshot,
    ) -> Result<ExternalValue<Self>, BufferAllocError> {
        ExternalValue::try_new_with(|| Self {
            app_state,
            snapshot,
            backlight_cycle_start: None,
            backlight_level: 0,
            frontlight_baseline: super::frontlight_policy::FRONTLIGHT_BASELINE_OFF,
            frontlight_calibration: None,
            wake_press: None,
            backlight_retry_at_ms: None,
            touch_startup_settled: false,
            runtime_ready_announced: false,
            gpio36_mode: if GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
                Gpio36Mode::ButtonOnly
            } else {
                Gpio36Mode::SharedWithTouch
            },
            presentation: super::presentation::PresentationState::new(),
            environment_delivery: super::environment_delivery::EnvironmentDeliveryState::default(),
            #[cfg(feature = "cpu-load")]
            cpu_delivery: super::cpu_delivery::CpuDeliveryState::default(),
            battery_delivery: super::battery_delivery::BatteryDeliveryState::default(),
        })
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

        // Persist before publishing so mode changes cannot precede their
        // durable state. This ordering alone does not quiesce an already
        // active radio during the flash operation.
        if result.persist_required() {
            console::println!("APPSTATE_SAVE begin");
            let mut persisted = context.app_state_store.load_state().unwrap_or_default();
            console::println!("APPSTATE_SAVE stage=load_done");
            persisted.update_from_snapshot(result.after);
            context.app_state_store.save_state(persisted);
            console::println!("APPSTATE_SAVE done");
            console::println!(
                "APPSTATE_SAVE stages flash={} store={}",
                super::super::flash::write_stage(),
                super::super::app_state::store::save_stage()
            );
        }

        self.snapshot = result.after;
        publish_app_state_snapshot(self.snapshot);
        if let Some(control) = result.diag_control() {
            DIAG_CONTROL_EVENTS.send(control).await;
        }

        if result.services_changed() {
            if result.before.services.upload_enabled && !result.after.services.upload_enabled {
                transfer_buffers::release_upload_chunk_buffer().await;
            }
        }

        result
    }
}
