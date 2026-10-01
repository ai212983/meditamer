//! Medinote's checked-LVGL implementations of [`shell::lifecycle::SurfaceRuntime`]
//! and [`shell::coordinator::CompositionRuntime`].

use core::ffi::CStr;

use flipclock::FlipClock as CounterFlipClock;
use medinote::apps::counter::CounterState;
use medinote::apps::descriptor::{MedinoteApp, MedinoteApps};
use medinote::catalogue;
use medinote::observations::HomeObservations;
use medinote::presentation::format_count;
#[cfg(feature = "cheertok-controls")]
use medinote::ui::overlay::ble_status;
use medinote::ui::overlay::settings;
#[cfg(feature = "wifi-storage")]
use medinote::ui::screen::network;
use medinote::ui::screen::{counter, home, hourglass, launcher};
use render::lvgl_adapter::UiAccessToken;
#[cfg(feature = "cheertok-controls")]
use shell::coordinator::CompositionDispatchOutcome;
use shell::coordinator::{CompositionRuntime, UiCoordinator};
use shell::lifecycle::{DestroyFailure, SurfaceRuntime};
use shell::navigator::NavigationFrame;
#[cfg(feature = "cheertok-controls")]
use shell::types::{
    CompositionIntent, OverlayAdmission, OverlayInput, OverlayLifetime, OwnedCompositionIntent,
};
use shell::types::{
    NavIntent, OverlayInstance, RefreshHint, SurfaceCapabilities, SurfaceInstanceToken, SurfaceRef,
    SurfaceRole, SurfaceSpec,
};

use crate::observations::{environment_now, sync_observation_demand};

pub(super) type RuntimeCoordinator =
    UiCoordinator<MedinoteScreen, MedinoteOverlay, 4, 8, 4, 2, 2, 4>;

pub(super) const BASE_SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec::new(
        catalogue::HOME_SURFACE_ID.0,
        SurfaceRole::Ambient,
        SurfaceCapabilities::AMBIENT,
        RefreshHint::Content,
    ),
    SurfaceSpec::new(
        catalogue::LAUNCHER_SURFACE_ID.0,
        SurfaceRole::Launcher,
        SurfaceCapabilities::NONE,
        RefreshHint::Content,
    ),
    #[cfg(feature = "cheertok-controls")]
    SurfaceSpec::new(
        catalogue::CHEERTOK_STATUS_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Micro,
    ),
    SurfaceSpec::new(
        catalogue::SETTINGS_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
];

/// Each navigable surface owns an LVGL root. Candidate construction leaves the
/// active origin untouched so `execute_transition` can roll back safely.
pub(super) enum MedinoteScreen {
    Home(home::Home),
    Launcher(launcher::Launcher),
    Hourglass(hourglass::Hourglass),
    Counter(counter::Counter),
    #[cfg(feature = "wifi-storage")]
    Network(network::Network),
}

impl MedinoteScreen {
    fn activate(&self, token: &UiAccessToken) -> bool {
        match self {
            Self::Home(screen) => screen.activate(token),
            Self::Launcher(screen) => screen.activate(token),
            Self::Hourglass(screen) => screen.activate(token),
            Self::Counter(screen) => screen.activate(token),
            #[cfg(feature = "wifi-storage")]
            Self::Network(screen) => screen.activate(token),
        }
    }
}

/// Builds destination roots without touching the active screen; `activate`
/// performs the visible swap for forward transitions and rollbacks.
/// `quiesce` and `enable` are trivial because KEY/BOOT bypass LVGL callbacks.
pub(super) struct MedinoteScreenRuntime<'a> {
    pub(super) ui_token: UiAccessToken,
    pub(super) counter_state: CounterState,
    /// Every product app, so an `AppRoot` destination is dispatched by
    /// *which app it resolves to* -- adding another `AppRoot` app only
    /// grows the container plus one match arm, never a change to
    /// `SurfaceRole::AppRoot` matching itself.
    pub(super) apps: &'a MedinoteApps,
    /// The product catalogue's own Launcher-entry labels, in view order --
    /// `Launcher::create`/`set_selected` render whatever this holds rather
    /// than a fixed one-app label.
    pub(super) launcher_labels: &'a [&'static CStr],
}

impl SurfaceRuntime for MedinoteScreenRuntime<'_> {
    type Instance = MedinoteScreen;
    type EnterError = ();

    fn enter(
        &mut self,
        frame: NavigationFrame,
        _token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError> {
        match frame.role {
            SurfaceRole::Ambient => home::create(&self.ui_token, c"Medinote", c"Home")
                .map(MedinoteScreen::Home)
                .ok_or(()),
            SurfaceRole::Launcher => launcher::create(&self.ui_token, self.launcher_labels)
                .map(MedinoteScreen::Launcher)
                .ok_or(()),
            SurfaceRole::AppRoot => {
                match self.apps.app_for_surface(frame.surface) {
                    Some(MedinoteApp::Hourglass) => hourglass::create(&self.ui_token)
                        .map(MedinoteScreen::Hourglass)
                        .ok_or(()),
                    Some(MedinoteApp::Counter) => {
                        let mut buffer = [0u8; 11];
                        let len = format_count(&mut buffer, self.counter_state.count());
                        let text = CStr::from_bytes_with_nul(&buffer[..len]).map_err(|_| ())?;
                        // Re-entering resumes the digit the clock settled on.
                        counter::create(&self.ui_token, text, self.counter_state.clock())
                            .map(MedinoteScreen::Counter)
                            .ok_or(())
                    }
                    #[cfg(feature = "wifi-storage")]
                    Some(MedinoteApp::Network) => {
                        let status = medinote::apps::network::NetworkStatus::Off;
                        let mut buffer = [0u8; 24];
                        let text = medinote::apps::network::format_status(&mut buffer, status);
                        network::create(&self.ui_token, text)
                            .map(MedinoteScreen::Network)
                            .ok_or(())
                    }
                    None => Err(()),
                }
            }
            // A future app may declare AppChild surfaces before this target
            // has learned how to construct them. Reject the candidate so the
            // coordinator can retain the active origin instead of panicking.
            _ => Err(()),
        }
    }

    fn activate(&mut self, instance: &Self::Instance) -> bool {
        instance.activate(&self.ui_token)
    }

    fn quiesce(&mut self, _instance: &Self::Instance) -> bool {
        true
    }

    fn enable(&mut self, _instance: &Self::Instance) -> bool {
        true
    }

    fn destroy(&mut self, instance: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        match instance {
            MedinoteScreen::Home(screen) => screen
                .destroy(&self.ui_token)
                .map_err(|screen| DestroyFailure::Live(MedinoteScreen::Home(screen))),
            MedinoteScreen::Launcher(screen) => screen
                .destroy(&self.ui_token)
                .map_err(|screen| DestroyFailure::Live(MedinoteScreen::Launcher(screen))),
            MedinoteScreen::Hourglass(screen) => screen
                .destroy(&self.ui_token)
                .map_err(|screen| DestroyFailure::Live(MedinoteScreen::Hourglass(screen))),
            MedinoteScreen::Counter(screen) => screen
                .destroy(&self.ui_token)
                .map_err(|screen| DestroyFailure::Live(MedinoteScreen::Counter(screen))),
            #[cfg(feature = "wifi-storage")]
            MedinoteScreen::Network(screen) => screen
                .destroy(&self.ui_token)
                .map_err(|screen| DestroyFailure::Live(MedinoteScreen::Network(screen))),
        }
    }
}

impl MedinoteScreenRuntime<'_> {
    pub(super) fn counter_clock(&self) -> &CounterFlipClock {
        self.counter_state.clock()
    }

    /// Advances the Counter flip animation. Returns whether it needs a repaint.
    pub(super) fn advance_counter(&mut self, delta_ms: u32) -> bool {
        self.counter_state.advance(delta_ms)
    }

    pub(super) fn increment_counter(&mut self) -> u32 {
        self.counter_state.increment()
    }
}

/// A live system-layer overlay built by [`MedinoteOverlayRuntime`].
pub(super) enum MedinoteOverlay {
    #[cfg(feature = "cheertok-controls")]
    BleStatus {
        instance: OverlayInstance,
        widget: ble_status::BleStatusChip,
    },
    Settings {
        instance: OverlayInstance,
        widget: settings::Settings,
    },
}

/// Infers the overlay kind from `instance.token.surface` because
/// `CompositionRuntime::enter` carries no separate kind parameter.
pub(super) struct MedinoteOverlayRuntime {
    pub(super) ui_token: UiAccessToken,
    pub(super) settings_surface: SurfaceRef,
}

impl CompositionRuntime for MedinoteOverlayRuntime {
    type Instance = MedinoteOverlay;
    type EnterError = ();

    #[cfg_attr(not(feature = "cheertok-controls"), allow(unused_variables))]
    fn enter(&mut self, instance: OverlayInstance) -> Result<Self::Instance, Self::EnterError> {
        if instance.token.surface == self.settings_surface {
            let widget = settings::create(&self.ui_token, instance).ok_or(())?;
            return Ok(MedinoteOverlay::Settings { instance, widget });
        }
        #[cfg(feature = "cheertok-controls")]
        {
            let widget = ble_status::create(&self.ui_token, instance, crate::cheertok::ui_state())
                .ok_or(())?;
            Ok(MedinoteOverlay::BleStatus { instance, widget })
        }
        #[cfg(not(feature = "cheertok-controls"))]
        {
            Err(())
        }
    }

    fn instance(&self, live: &Self::Instance) -> OverlayInstance {
        match live {
            #[cfg(feature = "cheertok-controls")]
            MedinoteOverlay::BleStatus { instance, .. } => *instance,
            MedinoteOverlay::Settings { instance, .. } => *instance,
        }
    }

    fn show(&mut self, live: &Self::Instance) {
        match live {
            #[cfg(feature = "cheertok-controls")]
            MedinoteOverlay::BleStatus { widget, .. } => widget.show(&self.ui_token),
            MedinoteOverlay::Settings { widget, .. } => widget.show(&self.ui_token),
        }
    }

    fn hide(&mut self, live: &Self::Instance) {
        match live {
            #[cfg(feature = "cheertok-controls")]
            MedinoteOverlay::BleStatus { widget, .. } => widget.hide(&self.ui_token),
            MedinoteOverlay::Settings { widget, .. } => widget.hide(&self.ui_token),
        }
    }

    fn enable(&mut self, _live: &Self::Instance) -> bool {
        true
    }

    fn disable(&mut self, _live: &Self::Instance) -> bool {
        true
    }

    fn destroy(&mut self, live: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        match live {
            #[cfg(feature = "cheertok-controls")]
            MedinoteOverlay::BleStatus { instance, widget } => {
                widget.destroy(&self.ui_token).map_err(|widget| {
                    DestroyFailure::Live(MedinoteOverlay::BleStatus { instance, widget })
                })
            }
            MedinoteOverlay::Settings { instance, widget } => {
                widget.destroy(&self.ui_token).map_err(|widget| {
                    DestroyFailure::Live(MedinoteOverlay::Settings { instance, widget })
                })
            }
        }
    }
}

/// Dispatches one atomic screen/overlay transition and applies observation
/// ownership only after commit. A retained cleanup fault rejects navigation
/// until the runtime loop completes its retry.
pub(super) fn navigate(
    coordinator: &mut RuntimeCoordinator,
    observations: &mut HomeObservations,
    intent: NavIntent,
    screen_runtime: &mut MedinoteScreenRuntime,
    overlay_runtime: &mut MedinoteOverlayRuntime,
) -> bool {
    let outcome = match coordinator.dispatch_navigation(intent, screen_runtime, overlay_runtime) {
        Ok(outcome) => outcome,
        Err(shell::coordinator::DispatchRejected::Faulted) => {
            console::println!("UI_NAV state=rejected reason=faulted");
            return false;
        }
        Err(rejected) => panic!("navigate: unexpected dispatch rejection {rejected:?}"),
    };
    let committed = matches!(
        outcome,
        shell::coordinator::NavigationDispatchOutcome::Committed { .. }
            | shell::coordinator::NavigationDispatchOutcome::FaultedAfterCommit
    );
    if !committed {
        console::println!("UI_NAV state=rolled_back outcome={outcome:?}");
        return false;
    }
    if observations
        .committed(coordinator.shell().active_instance(), environment_now())
        .is_err()
    {
        console::println!("HOME_OBSERVATIONS_DISABLED reason=owner_generation_exhausted");
    }
    sync_observation_demand(observations);
    true
}

/// Admits the CheerTok status overlay through the coordinator and returns
/// its instance token, so the runtime loop can reach it later (for the
/// per-frame `.update()` calls that are not themselves a lifecycle
/// transition) via [`shell::coordinator::UiCoordinator::live_overlay_mut`].
#[cfg(feature = "cheertok-controls")]
pub(super) fn install_ble_status_overlay(
    coordinator: &mut RuntimeCoordinator,
    overlay_runtime: &mut MedinoteOverlayRuntime,
    surface: SurfaceRef,
) -> SurfaceInstanceToken {
    let source = coordinator.shell().active_instance();
    let outcome = coordinator
        .dispatch_composition(
            OwnedCompositionIntent {
                source,
                intent: CompositionIntent::Request {
                    surface,
                    input: OverlayInput::Passive,
                    lifetime: OverlayLifetime::Sticky,
                    rank: 1,
                },
            },
            overlay_runtime,
        )
        .expect("coordinator is not faulted at boot");
    let CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(instance)) = outcome else {
        panic!("CheerTok status overlay commit: {outcome:?}");
    };
    instance.token
}
