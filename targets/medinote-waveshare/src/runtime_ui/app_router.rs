//! Catalogue-backed app selection and active-app dispatch for the UI loop.

use core::ffi::CStr;

use hourglass::model::PHYSICS_HZ;
use medinote::apps::descriptor::{MedinoteApp, MedinoteApps};
#[cfg(feature = "wifi-storage")]
use medinote::apps::network::NetworkStatus;
use medinote::catalogue::MedinoteCatalogue;
use medinote::controls::{merge_rotation_inputs, RotationControl, RotationInput};
use medinote::input::{key_action, KeyAction, KeyEdges};
use medinote::observations::HomeObservations;
use medinote::power::RuntimePowerMode;
use medinote::presentation::format_count;
use render::lvgl_adapter::UiAccessToken;
use shell::catalogue::{CatalogueAction, CatalogueViewKind};
use shell::types::{SurfaceRef, SurfaceRole};
use waveshare_rlcd42::panel_lvgl::PanelLvglSession;

use super::hourglass_runtime::HourglassRuntime;
use super::power;
use super::surfaces::{
    navigate, MedinoteOverlayRuntime, MedinoteScreen, MedinoteScreenRuntime, RuntimeCoordinator,
};
use super::telemetry::RuntimeTelemetry;

const ROTATION_INPUTS_PER_TICK_MAX: usize = 17;

pub(super) type RotationInputs = heapless::Vec<RotationInput, ROTATION_INPUTS_PER_TICK_MAX>;

pub(super) struct KeyRoutingContext<'borrow, 'apps> {
    pub(super) coordinator: &'borrow mut RuntimeCoordinator,
    pub(super) observations: &'borrow mut HomeObservations,
    pub(super) screen_runtime: &'borrow mut MedinoteScreenRuntime<'apps>,
    pub(super) overlay_runtime: &'borrow mut MedinoteOverlayRuntime,
}

/// One runtime tick, in milliseconds -- the loop in `mod.rs` runs at
/// `PHYSICS_HZ`, and the flip animation is advanced by it.
const TICK_MS: u32 = 1000 / PHYSICS_HZ;

pub(super) struct ActiveAppContext<'borrow, 'apps> {
    pub(super) coordinator: &'borrow RuntimeCoordinator,
    pub(super) screen_runtime: &'borrow mut MedinoteScreenRuntime<'apps>,
    pub(super) runtime_mode: &'borrow mut RuntimePowerMode,
    pub(super) panel_session: &'borrow mut PanelLvglSession,
    pub(super) key_consumed: bool,
    pub(super) key_pressed_at_us: Option<u64>,
    pub(super) rotation_inputs: RotationInputs,
    pub(super) telemetry: &'borrow mut RuntimeTelemetry,
}

#[derive(Clone, Copy, Default)]
pub(super) struct KeyRoutingOutcome {
    pub(super) consumed: bool,
    pub(super) screen_dirty: bool,
}

pub(super) struct AppRouter<'a> {
    ui_token: UiAccessToken,
    product_catalogue: &'a MedinoteCatalogue,
    apps: &'a MedinoteApps,
    launcher_labels: &'a [&'static CStr],
    launcher_surface: SurfaceRef,
    launcher_selection: usize,
    hourglass: HourglassRuntime,
    #[cfg(feature = "wifi-storage")]
    network_status: Option<NetworkStatus>,
}

impl<'a> AppRouter<'a> {
    pub(super) fn new(
        ui_token: UiAccessToken,
        product_catalogue: &'a MedinoteCatalogue,
        apps: &'a MedinoteApps,
        launcher_labels: &'a [&'static CStr],
        launcher_surface: SurfaceRef,
        hourglass: HourglassRuntime,
    ) -> Self {
        Self {
            ui_token,
            product_catalogue,
            apps,
            launcher_labels,
            launcher_surface,
            launcher_selection: 0,
            hourglass,
            #[cfg(feature = "wifi-storage")]
            network_status: None,
        }
    }

    pub(super) fn collect_rotation_inputs() -> RotationInputs {
        let inputs = RotationInputs::new();
        #[cfg(feature = "cheertok-controls")]
        {
            let mut inputs = inputs;
            for input in crate::cheertok::drain_rotation_inputs() {
                let _ = inputs.push(input);
            }
            inputs
        }
        #[cfg(not(feature = "cheertok-controls"))]
        {
            inputs
        }
    }

    pub(super) fn route_key(
        &mut self,
        key_edges: KeyEdges,
        context: KeyRoutingContext<'_, '_>,
    ) -> KeyRoutingOutcome {
        let role = context.coordinator.shell().active().role;
        let launcher_view = self.product_catalogue.view(CatalogueViewKind::Launcher);
        let launcher_entries = launcher_view.entries();
        let selected_index = if launcher_entries.is_empty() {
            0
        } else {
            self.launcher_selection % launcher_entries.len()
        };
        let launch_target = launcher_entries
            .get(selected_index)
            .and_then(|entry| match entry.action() {
                CatalogueAction::Enter(surface) => Some(surface),
                CatalogueAction::Unavailable(_) => None,
            })
            .unwrap_or(self.launcher_surface);

        match (
            role,
            key_action(role, key_edges, self.launcher_surface, launch_target),
        ) {
            (SurfaceRole::Ambient, KeyAction::Navigate(intent)) => {
                let committed = navigate(
                    context.coordinator,
                    context.observations,
                    intent,
                    context.screen_runtime,
                    context.overlay_runtime,
                );
                console::println!(
                    "UI_SURFACE role={:?}",
                    context.coordinator.shell().active().role
                );
                KeyRoutingOutcome {
                    consumed: true,
                    screen_dirty: committed,
                }
            }
            (SurfaceRole::Launcher, KeyAction::CycleLauncherSelection) => {
                if !launcher_entries.is_empty() {
                    self.launcher_selection =
                        (self.launcher_selection + 1) % launcher_entries.len();
                }
                if let Some(MedinoteScreen::Launcher(screen)) = context.coordinator.active_screen()
                {
                    screen.set_selected(
                        &self.ui_token,
                        self.launcher_labels,
                        self.launcher_selection,
                    );
                }
                KeyRoutingOutcome {
                    consumed: false,
                    screen_dirty: true,
                }
            }
            (SurfaceRole::Launcher, KeyAction::Navigate(intent)) => {
                let committed = navigate(
                    context.coordinator,
                    context.observations,
                    intent,
                    context.screen_runtime,
                    context.overlay_runtime,
                );
                if committed {
                    match self
                        .apps
                        .app_for_surface(context.coordinator.shell().active().surface)
                    {
                        Some(MedinoteApp::Hourglass) => self.hourglass.activate(),
                        Some(MedinoteApp::Counter) | None => {}
                        #[cfg(feature = "wifi-storage")]
                        Some(MedinoteApp::Network) => {
                            self.network_status = None;
                        }
                    }
                    console::println!(
                        "UI_SURFACE role={:?}",
                        context.coordinator.shell().active().role
                    );
                }
                KeyRoutingOutcome {
                    consumed: true,
                    screen_dirty: committed,
                }
            }
            #[cfg(feature = "wifi-storage")]
            (SurfaceRole::AppRoot, KeyAction::AppInput)
                if self
                    .apps
                    .app_for_surface(context.coordinator.shell().active().surface)
                    == Some(MedinoteApp::Network) =>
            {
                let line: &[u8] = if crate::net_host::is_enabled() {
                    b"NET STOP\n"
                } else {
                    b"NET START\n"
                };
                let consumed = crate::net_commands::handle_line(line);
                KeyRoutingOutcome {
                    consumed,
                    screen_dirty: consumed,
                }
            }
            (SurfaceRole::AppRoot, KeyAction::AppInput) | (_, KeyAction::None) => {
                KeyRoutingOutcome::default()
            }
            (role, action) => {
                unreachable!(
                    "key_action({role:?}, ..) returned {action:?}, which no arm here handles"
                )
            }
        }
    }

    pub(super) fn tick_active(&mut self, mut context: ActiveAppContext<'_, '_>) {
        if let Some(MedinoteScreen::Hourglass(widget)) = context.coordinator.active_screen() {
            let local_input = (!context.key_consumed)
                .then_some(context.key_pressed_at_us)
                .flatten()
                .map(|ticks_us| RotationInput {
                    control: RotationControl::Clockwise,
                    pressed: true,
                    ticks_us,
                });
            merge_rotation_inputs(&mut context.rotation_inputs, local_input);
            let desired = self.hourglass.tick(
                &self.ui_token,
                widget,
                context.rotation_inputs.as_slice(),
                context.panel_session,
                context.telemetry,
            );
            power::set_runtime_power(
                context.runtime_mode,
                desired,
                context.panel_session,
                "hourglass-state",
            );
        }

        if let Some(MedinoteScreen::Counter(widget)) = context.coordinator.active_screen() {
            // One press runs a whole flip. The animation is then driven from
            // this runtime tick rather than from further input, so speed and
            // easing live in the `flipclock` crate (FLIP_DURATION_MS,
            // FLIP_EASING) instead of being implied by how fast KEY is pressed.
            let pressed = !context.key_consumed && context.key_pressed_at_us.is_some();
            let flush_before = waveshare_rlcd42::panel::flush_completion();

            if pressed {
                let mut buffer = [0u8; 11];
                let len = format_count(&mut buffer, context.screen_runtime.increment_counter());
                if let Ok(text) = CStr::from_bytes_with_nul(&buffer[..len]) {
                    widget.set_count(&self.ui_token, text);
                }
            }

            // Advance first, then repaint once: the label only changes on a
            // press, so an animating frame dirties the canvas alone rather
            // than the union of canvas and label.
            let animating = context.screen_runtime.advance_counter(TICK_MS);
            if pressed || animating {
                widget.set_flip(&self.ui_token, context.screen_runtime.counter_clock());
                let _ = self.ui_token.refresh_default_display();
            }

            if pressed {
                if let Some(input_at_us) = context.key_pressed_at_us {
                    context.telemetry.record_input_flush(
                        input_at_us,
                        flush_before,
                        waveshare_rlcd42::panel::flush_completion(),
                    );
                }
            }
        }

        #[cfg(feature = "wifi-storage")]
        if let Some(MedinoteScreen::Network(widget)) = context.coordinator.active_screen() {
            let snapshot = netstack::wifi::net_status_snapshot();
            let status = if !crate::net_host::is_enabled() {
                NetworkStatus::Off
            } else if snapshot.state == "Ready" && snapshot.ipv4 != [0; 4] {
                NetworkStatus::Ready(snapshot.ipv4)
            } else if snapshot.state == "Failed" {
                NetworkStatus::Failed
            } else {
                NetworkStatus::Connecting
            };
            if self.network_status != Some(status) {
                let mut buffer = [0u8; 24];
                let text = medinote::apps::network::format_status(&mut buffer, status);
                if widget.set_status(&self.ui_token, text) {
                    self.network_status = Some(status);
                    let _ = self.ui_token.refresh_default_display();
                }
            }
        }
    }
}
