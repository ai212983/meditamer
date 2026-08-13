use embassy_time::Instant;

use super::super::{
    app_state::{
        publish_app_state_snapshot, AppStateApplyResult, AppStateCommand, AppStateEngine,
        AppStateSnapshot,
    },
    config::DIAG_CONTROL_EVENTS,
    input::gpio36::Gpio36Mode,
    storage::transfer_buffers,
    touch::config::GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED,
    types::DisplayContext,
};

pub(super) struct DisplayLoopState {
    pub(super) app_state: AppStateEngine,
    pub(super) snapshot: AppStateSnapshot,
    pub(super) backlight_cycle_start: Option<Instant>,
    pub(super) backlight_level: u8,
    pub(super) touch_startup_settled: bool,
    pub(super) runtime_ready_announced: bool,
    pub(super) gpio36_mode: Gpio36Mode,
    pub(super) presentation: super::presentation::PresentationState,
}

impl DisplayLoopState {
    pub(super) async fn new(context: &mut DisplayContext) -> Self {
        let persisted = context.app_state_store.load_state().unwrap_or_default();
        let mut app_state = AppStateEngine::from_persisted(persisted);
        let boot_result = app_state.apply(AppStateCommand::BootComplete);
        if let Some(control) = boot_result.diag_control() {
            DIAG_CONTROL_EVENTS.send(control).await;
        }
        let snapshot = app_state.snapshot();
        publish_app_state_snapshot(snapshot);

        Self {
            app_state,
            snapshot,
            backlight_cycle_start: None,
            backlight_level: 0,
            touch_startup_settled: false,
            runtime_ready_announced: false,
            gpio36_mode: if GPIO36_WAKE_BUTTON_DIAGNOSTIC_ENABLED {
                Gpio36Mode::ButtonOnly
            } else {
                Gpio36Mode::SharedWithTouch
            },
            presentation: super::presentation::PresentationState::new(),
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
            let mut persisted = context.app_state_store.load_state().unwrap_or_default();
            persisted.update_from_snapshot(result.after);
            context.app_state_store.save_state(persisted);
        }

        if result.services_changed() {
            if result.before.services.upload_enabled && !result.after.services.upload_enabled {
                transfer_buffers::release_upload_chunk_buffer().await;
            }
            if !result.before.services.upload_enabled && result.after.services.upload_enabled {
                let _ = context.inkplate.frontlight_off().await;
            }
        }

        result
    }

    pub(super) fn upload_enabled(&self) -> bool {
        self.snapshot.services.upload_enabled
    }
}
