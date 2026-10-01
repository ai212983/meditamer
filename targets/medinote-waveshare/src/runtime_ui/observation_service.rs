//! Home observation, console-fixture, and Settings-request orchestration.

use medinote::observations::{fixture::ConsoleInput, HomeObservations};
use render::lvgl_adapter::UiAccessToken;
use shell::types::{
    CompositionIntent, NavIntent, OverlayInput, OverlayLifetime, OwnedCompositionIntent,
    SurfaceRef, SurfaceRole,
};

use super::surfaces::{
    navigate, MedinoteOverlayRuntime, MedinoteScreen, MedinoteScreenRuntime, RuntimeCoordinator,
};
use crate::observations::{
    deactivate_observations, environment_now, poll_observations, sync_observation_demand,
    PollObservationsContext, SettingsOverlayRequest,
};

pub(super) struct ObservationService {
    observations: HomeObservations,
    console: ConsoleInput,
    console_ready: bool,
    pending_sleep: Option<crate::sleep_fixture::PendingSleep>,
    settings_surface: SurfaceRef,
}

impl ObservationService {
    pub(super) fn new(settings_surface: SurfaceRef, coordinator: &RuntimeCoordinator) -> Self {
        let mut observations = HomeObservations::new();
        if observations
            .committed(coordinator.shell().active_instance(), environment_now())
            .is_err()
        {
            console::println!("HOME_OBSERVATIONS_DISABLED reason=owner_generation_exhausted");
        }
        sync_observation_demand(&observations);
        Self {
            observations,
            console: ConsoleInput::default(),
            console_ready: false,
            pending_sleep: None,
            settings_surface,
        }
    }

    pub(super) fn observations_mut(&mut self) -> &mut HomeObservations {
        &mut self.observations
    }

    pub(super) fn deactivate(&mut self) {
        deactivate_observations(&mut self.observations);
    }

    pub(super) fn begin_sleep_fixture(
        &mut self,
        coordinator: &mut RuntimeCoordinator,
        screen_runtime: &mut MedinoteScreenRuntime<'_>,
        overlay_runtime: &mut MedinoteOverlayRuntime,
    ) -> Option<crate::sleep_fixture::PendingSleep> {
        let fixture = self
            .pending_sleep
            .take()
            .and_then(|request| request.begin());
        if fixture.is_some() {
            navigate(
                coordinator,
                &mut self.observations,
                NavIntent::Home,
                screen_runtime,
                overlay_runtime,
            );
        }
        fixture
    }

    pub(super) fn poll(
        &mut self,
        ui_token: &UiAccessToken,
        coordinator: &mut RuntimeCoordinator,
        overlay_runtime: &mut MedinoteOverlayRuntime,
        jtag_owned_elsewhere: bool,
    ) -> bool {
        if !self.console_ready {
            console::println!("RUNTIME_READY app_state=ready display=ready");
            self.console_ready = true;
        }
        let home = coordinator.active_screen().and_then(|screen| match screen {
            MedinoteScreen::Home(home) => Some(home),
            MedinoteScreen::Launcher(_)
            | MedinoteScreen::Hourglass(_)
            | MedinoteScreen::Counter(_) => None,
            #[cfg(feature = "wifi-storage")]
            MedinoteScreen::Network(_) => None,
        });
        let (observations_changed, sleep_request, settings_request) =
            poll_observations(PollObservationsContext {
                observations: &mut self.observations,
                input: &mut self.console,
                ui_token,
                home,
                jtag_owned_elsewhere,
            });
        self.pending_sleep = sleep_request.map(crate::sleep_fixture::PendingSleep::new);
        self.dispatch_settings(settings_request, coordinator, overlay_runtime)
            || observations_changed
    }

    fn dispatch_settings(
        &self,
        request: SettingsOverlayRequest,
        coordinator: &mut RuntimeCoordinator,
        overlay_runtime: &mut MedinoteOverlayRuntime,
    ) -> bool {
        match request {
            SettingsOverlayRequest::Open => {
                if coordinator.shell().active().role != SurfaceRole::Ambient
                    || coordinator.shell().active_modal().is_some()
                {
                    console::println!(
                        "UI_SETTINGS state=rejected reason=not_home_or_modal_active role={:?}",
                        coordinator.shell().active().role,
                    );
                    return false;
                }
                let source = coordinator.shell().active_instance();
                let outcome = coordinator.dispatch_composition(
                    OwnedCompositionIntent {
                        source,
                        intent: CompositionIntent::Request {
                            surface: self.settings_surface,
                            input: OverlayInput::Modal,
                            lifetime: OverlayLifetime::Transient,
                            rank: 3,
                        },
                    },
                    overlay_runtime,
                );
                console::println!("UI_SETTINGS state=requested outcome={:?}", outcome);
                true
            }
            SettingsOverlayRequest::Close => {
                let Some(modal) = coordinator.shell().active_modal() else {
                    console::println!("UI_SETTINGS state=rejected reason=no_active_modal");
                    return false;
                };
                let outcome = coordinator.dispatch_composition(
                    OwnedCompositionIntent {
                        source: modal.token,
                        intent: CompositionIntent::DismissActiveModal,
                    },
                    overlay_runtime,
                );
                console::println!("UI_SETTINGS state=dismissed outcome={:?}", outcome);
                true
            }
            SettingsOverlayRequest::None => false,
        }
    }
}
