//! LVGL backend.
//!
//! This file owns the backend's data model -- the surface set, the active
//! surface and overlay state, and the [`Backend`] handle itself. The work is
//! split by responsibility: [`init`] brings LVGL up, [`frame`] drives one
//! frame, [`navigation`] and [`overlay`] advance the shell's surface graph,
//! and [`cycle`] serves the serial-driven UI cycle and its fixture.

#![forbid(unsafe_code)]

mod cycle;
mod frame;
mod init;
#[cfg(feature = "ui-initialization-fixture")]
mod initialization_fixture;
mod navigation;
mod overlay;
mod scheduling;
mod transition_readiness;

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::firmware::environment::{EnvironmentSnapshot, EnvironmentStateSnapshot};
use embassy_time::Instant;
use transition_readiness::{
    needs_transition_boundary, next_transition_rendered, InputTransitionReadiness,
};

use crate::firmware::ui::apps::{
    self, MeditamerApp, MeditamerApps, AMBIENT_VIEW_ENTRY_ID, AMBIENT_VIEW_SURFACE,
    AMBIENT_VIEW_SURFACE_ID, ANALOG_CLOCK_SURFACE, ANALOG_CLOCK_SURFACE_ID, BASE_PROVIDER_ID,
    DIAGNOSTICS_SURFACE, DIAGNOSTICS_SURFACE_ID, NAMESPACE, OVERLAY_TOGGLES_SURFACE,
    OVERLAY_TOGGLES_SURFACE_ID,
};

use super::{
    io,
    screen_update_state::{ClockRemovalState, PendingFastClock},
    HEIGHT, WIDTH,
};
use crate::firmware::ui::overlay::base_overlays::{
    ActiveOverlay, BaseOverlayKind, OverlayEnterError,
};
pub(crate) use crate::firmware::ui::overlay::frontlight_calibration::FrontlightCalibrationEffect;
#[cfg(feature = "ui-provider-fixture")]
use crate::firmware::ui::screen::provider_fixture;
use crate::firmware::ui::screen::{
    ambient_view, analog_clock, gesture_test, home, launcher, overlay_toggles,
};
use crate::firmware::{
    observability,
    psram::{self, BufferPlacement},
    touch::lvgl_multitouch::{LvglContactBatch, LvglMultitouchFrame, LvglMultitouchTracker},
    touch::tasks::request_touch_pipeline_reset,
    touch::types::TouchEvent,
    types::InkplateDriver,
};
use render::intent_bridge;
use render::lvgl_adapter::{
    Display, PointerInput, PointerInputAccess, RuntimeSession, StaticL8DrawBuffer, UiAccessToken,
    Widget,
};
use render::DirtyArea;
#[cfg(feature = "ui-provider-fixture")]
use shell::coordinator::ProviderRemovalDispatchOutcome;
#[cfg(feature = "ui-provider-fixture")]
use shell::model::{PendingProviderRemoval, ProviderRuntimeAudit};
#[cfg(feature = "ui-provider-fixture")]
use shell::types::ProviderId;
use shell::{
    catalogue::{CatalogueAction, CatalogueViewKind, DefaultCatalogue, EntryId},
    coordinator::{
        CompositionDispatchOutcome, CompositionRollbackReason, CompositionRuntime,
        NavigationDispatchOutcome, UiCoordinator,
    },
    lifecycle::{DestroyFailure, LifecycleEvent, SurfaceRuntime},
    model::{
        LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY, NAVIGATION_STACK_CAPACITY, PROVIDER_CAPACITY,
        SHELL_INTENT_QUEUE_CAPACITY, SURFACE_REGISTRY_CAPACITY,
    },
    navigator::NavigationFrame,
    settings::{PersistedUiSettings, UiSettings, UiSettingsPersistence},
    timing::TimerServiceMetrics,
    types::{
        CompositionIntent, NavIntent, OverlayAdmission, OverlayInput, OverlayInstance,
        OverlayLifetime, OwnedCompositionIntent, OwnedNavIntent, OwnedScreenUpdateRequest,
        OwnedShellIntent, OwnedUiSettingsIntent, RefreshHint, ScreenUpdate, ScreenUpdateIntent,
        ScreenUpdateRequest, SurfaceCapabilities, SurfaceId, SurfaceInstanceToken, SurfaceRef,
        SurfaceRole, SurfaceSpec,
    },
};

const BUFFER_LINES: usize = 16;
const BUFFER_BYTES: usize = WIDTH as usize * BUFFER_LINES;
const BUFFER_WORDS: usize = BUFFER_BYTES.div_ceil(core::mem::size_of::<u32>());
const MEMORY_POOL_BYTES: usize = 128 * 1024;
const HOME_SURFACE_ID: SurfaceId = SurfaceId(1);
const LAUNCHER_SURFACE_ID: SurfaceId = SurfaceId(2);
const NAVIGATION_CUE_SURFACE_ID: SurfaceId = SurfaceId(4);
const STICKY_STATUS_SURFACE_ID: SurfaceId = SurfaceId(5);
const CONFIRM_SURFACE_ID: SurfaceId = SurfaceId(6);
const SETTINGS_SURFACE_ID: SurfaceId = SurfaceId(9);
const CLOCK_OVERLAY_SURFACE_ID: SurfaceId = SurfaceId(10);
const FRONTLIGHT_CALIBRATION_SURFACE_ID: SurfaceId = SurfaceId(12);
const HOME_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 1);
const REFRESH_CONTROL_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 5);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ID: ProviderId = ProviderId(2);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ROOT_ID: SurfaceId = SurfaceId(101);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_OVERLAY_ID: SurfaceId = SurfaceId(102);
const BASE_SURFACES: [SurfaceSpec; 12] = [
    SurfaceSpec::new(
        HOME_SURFACE_ID.0,
        SurfaceRole::Ambient,
        SurfaceCapabilities::AMBIENT,
        RefreshHint::Boundary,
    ),
    SurfaceSpec::new(
        LAUNCHER_SURFACE_ID.0,
        SurfaceRole::Launcher,
        SurfaceCapabilities::NONE,
        RefreshHint::Boundary,
    ),
    DIAGNOSTICS_SURFACE,
    SurfaceSpec::new(
        NAVIGATION_CUE_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Micro,
    ),
    SurfaceSpec::new(
        STICKY_STATUS_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Micro,
    ),
    SurfaceSpec::new(
        CONFIRM_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
    AMBIENT_VIEW_SURFACE,
    ANALOG_CLOCK_SURFACE,
    OVERLAY_TOGGLES_SURFACE,
    SurfaceSpec::new(
        SETTINGS_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
    SurfaceSpec::new(
        CLOCK_OVERLAY_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
    SurfaceSpec::new(
        FRONTLIGHT_CALIBRATION_SURFACE_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
];

const _: () = {
    let mut current = 0;
    while current < BASE_SURFACES.len() {
        let mut other = current + 1;
        while other < BASE_SURFACES.len() {
            assert!(BASE_SURFACES[current].id.0 != BASE_SURFACES[other].id.0);
            other += 1;
        }
        current += 1;
    }
};
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_SURFACES: [SurfaceSpec; 2] = [
    SurfaceSpec::new(
        PROVIDER_FIXTURE_ROOT_ID.0,
        SurfaceRole::AppRoot,
        SurfaceCapabilities::LAUNCHABLE,
        RefreshHint::Boundary,
    ),
    SurfaceSpec::new(
        PROVIDER_FIXTURE_OVERLAY_ID.0,
        SurfaceRole::Overlay,
        SurfaceCapabilities::OVERLAY,
        RefreshHint::Content,
    ),
];

// LVGL 9.5.4 declares this as the rotation recognizer's default, but its lazy
// initialization leaves the configuration zeroed. Set it explicitly so touch
// jitter cannot win recognition before a two-finger swipe reaches its limit.
const ROTATION_THRESHOLD_RADIANS: f32 = 0.2;

static MEMORY_POOL: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitError {
    AlreadyInitialized,
    MemoryPoolUnavailable,
    DisplayCreationFailed,
    InputCreationFailed,
    ShellConfigurationFailed,
    SurfaceCreationFailed,
    SurfaceActivationFailed,
    SurfaceCleanupFailed,
    CallbackRouteUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiCycleStepError {
    Busy,
    NavigationFault,
    NoDirty,
}

#[derive(Clone, Copy)]
struct SurfaceRefs {
    home: SurfaceRef,
    launcher: SurfaceRef,
    diagnostics: SurfaceRef,
    ambient_view: SurfaceRef,
    analog_clock: SurfaceRef,
    overlay_toggles: SurfaceRef,
    navigation_cue: SurfaceRef,
    sticky_status: SurfaceRef,
    confirm: SurfaceRef,
    settings: SurfaceRef,
    clock_overlay: SurfaceRef,
    frontlight_calibration: SurfaceRef,
    #[cfg(feature = "ui-provider-fixture")]
    provider_fixture: SurfaceRef,
    #[cfg(feature = "ui-provider-fixture")]
    provider_overlay: SurfaceRef,
}

enum SurfaceModel {
    Home(home::HomeScreen),
    Launcher(launcher::LauncherScreen),
    Diagnostics(gesture_test::GestureTestScreen),
    AmbientView(ambient_view::AmbientViewScreen),
    AnalogClock(analog_clock::AnalogClockScreen),
    OverlayToggles(overlay_toggles::OverlayTogglesScreen),
    #[cfg(feature = "ui-provider-fixture")]
    ProviderFixture(provider_fixture::ProviderFixtureScreen),
}

#[cfg(feature = "ui-provider-fixture")]
enum ProviderFixtureState {
    Registered(shell::types::ProviderToken),
    Detaching(PendingProviderRemoval),
    Removed,
}

/// The renderer-independent coordinator, specialized to LVGL's own screen
/// and overlay instance types and to `DefaultShellModel`'s capacities --
/// this backend owns exactly one `ShellModel`, reached only through the
/// coordinator.
type MeditamerCoordinator = UiCoordinator<
    ActiveSurface,
    ActiveOverlay,
    PROVIDER_CAPACITY,
    SURFACE_REGISTRY_CAPACITY,
    NAVIGATION_STACK_CAPACITY,
    LIVE_OVERLAY_CAPACITY,
    MODAL_QUEUE_CAPACITY,
    SHELL_INTENT_QUEUE_CAPACITY,
>;

struct ActiveSurface {
    frame: NavigationFrame,
    token: SurfaceInstanceToken,
    callbacks: intent_bridge::CallbackLease,
    model: SurfaceModel,
}

impl ActiveSurface {
    fn enter(
        frame: NavigationFrame,
        token: SurfaceInstanceToken,
        runtime: &mut LvglSurfaceRuntime<'_>,
    ) -> Result<Self, InitError> {
        let LvglSurfaceRuntime {
            surfaces,
            apps,
            catalogue,
            settings,
            ui_token,
            clock_shared,
            ambient_shared,
            ..
        } = runtime;
        let surfaces = *surfaces;
        // Home requests Settings through its modal-request slot. Every
        // other screen requests the shared Confirm dialog (or, under the
        // fixture, the provider's own overlay).
        let requested_overlay = if frame.surface == surfaces.home {
            surfaces.settings
        } else {
            #[cfg(feature = "ui-provider-fixture")]
            {
                if frame.surface == surfaces.provider_fixture {
                    surfaces.provider_overlay
                } else {
                    surfaces.confirm
                }
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                surfaces.confirm
            }
        };
        let mut actions = [None; intent_bridge::SCREEN_NAVIGATION_CAPACITY];
        actions[intent_bridge::HOME_NAVIGATION_INDEX] =
            Some(intent_bridge::ScreenAction::Navigate(NavIntent::Home));
        actions[intent_bridge::BACK_NAVIGATION_INDEX] =
            Some(intent_bridge::ScreenAction::Navigate(NavIntent::Back));
        if frame.surface == surfaces.home {
            actions[0] = Some(intent_bridge::ScreenAction::Navigate(
                NavIntent::OpenLauncher(surfaces.launcher),
            ));
        } else if frame.surface == surfaces.launcher {
            for (index, entry) in catalogue
                .view(CatalogueViewKind::Launcher)
                .entries()
                .iter()
                .enumerate()
            {
                actions[index] = match entry.action() {
                    CatalogueAction::Enter(surface) => Some(intent_bridge::ScreenAction::Navigate(
                        NavIntent::Launch(surface),
                    )),
                    CatalogueAction::Unavailable(_) => None,
                };
            }
        } else if frame.surface == surfaces.overlay_toggles {
            for (index, entry) in catalogue
                .view(CatalogueViewKind::OverlayToggles)
                .entries()
                .iter()
                .enumerate()
            {
                if matches!(entry.action(), CatalogueAction::Enter(_)) {
                    actions[index] = Some(intent_bridge::ScreenAction::Configure(
                        shell::settings::UiSettingsIntent::ToggleOverlay(entry.id),
                    ));
                }
            }
        }
        let bindings = intent_bridge::IntentBindings::Screen {
            source: token,
            actions,
            show_confirm: CompositionIntent::Request {
                surface: requested_overlay,
                input: OverlayInput::Modal,
                // Settings is dismissed on any navigation away from Home,
                // same as Confirm; only the provider fixture's own overlay
                // is Sticky (it must survive the modal-preemption sequence
                // that fixture drives).
                lifetime: if requested_overlay == surfaces.confirm
                    || requested_overlay == surfaces.settings
                {
                    OverlayLifetime::Transient
                } else {
                    OverlayLifetime::Sticky
                },
                rank: 3,
            },
        };
        let callbacks =
            intent_bridge::claim(bindings).map_err(|_| InitError::CallbackRouteUnavailable)?;
        let model = {
            if frame.surface == surfaces.home {
                home::create(ui_token, &callbacks).map(SurfaceModel::Home)
            } else if frame.surface == surfaces.launcher {
                launcher::create(ui_token, catalogue, settings, &callbacks)
                    .map(SurfaceModel::Launcher)
            } else if let Some(app) = apps.app_for_surface(frame.surface) {
                match app {
                    MeditamerApp::GestureDiagnostics => {
                        gesture_test::create(ui_token, &callbacks).map(SurfaceModel::Diagnostics)
                    }
                    MeditamerApp::AmbientView => ambient_view::create(ui_token, ambient_shared)
                        .map(SurfaceModel::AmbientView),
                    MeditamerApp::AnalogClock => {
                        analog_clock::create(ui_token, token, clock_shared)
                            .map(SurfaceModel::AnalogClock)
                    }
                    MeditamerApp::OverlayToggles => {
                        overlay_toggles::create(ui_token, catalogue, settings, &callbacks)
                            .map(SurfaceModel::OverlayToggles)
                    }
                }
            } else if cfg!(feature = "ui-provider-fixture")
                && frame.surface == {
                    #[cfg(feature = "ui-provider-fixture")]
                    {
                        surfaces.provider_fixture
                    }
                    #[cfg(not(feature = "ui-provider-fixture"))]
                    {
                        surfaces.home
                    }
                }
            {
                #[cfg(feature = "ui-provider-fixture")]
                {
                    provider_fixture::create(ui_token, &callbacks)
                        .map(SurfaceModel::ProviderFixture)
                }
                #[cfg(not(feature = "ui-provider-fixture"))]
                {
                    None
                }
            } else {
                None
            }
        };
        let Some(model) = model else {
            let _ = intent_bridge::release(&callbacks);
            return Err(InitError::SurfaceCreationFailed);
        };
        Ok(Self {
            frame,
            token,
            callbacks,
            model,
        })
    }

    pub(super) fn root_widget(&self) -> &Widget {
        match &self.model {
            SurfaceModel::Home(screen) => screen.root_widget(),
            SurfaceModel::Launcher(screen) => screen.root_widget(),
            SurfaceModel::Diagnostics(screen) => screen.root_widget(),
            SurfaceModel::AmbientView(screen) => screen.root_widget(),
            SurfaceModel::AnalogClock(screen) => screen.root_widget(),
            SurfaceModel::OverlayToggles(screen) => screen.root_widget(),
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(screen) => screen.root_widget(),
        }
    }

    pub(super) fn activate(&self, ui_token: &UiAccessToken) -> bool {
        self.root_widget().activate(ui_token).unwrap_or(false)
    }

    pub(super) fn enable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::enable(&self.callbacks)
    }

    pub(super) fn disable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::disable(&self.callbacks)
    }

    pub(super) fn destroy(self, ui_token: &UiAccessToken) -> Result<(), DestroyFailure<Self>> {
        if self.disable().is_err() {
            return Err(DestroyFailure::Live(self));
        }
        intent_bridge::purge_instance(self.token);
        let Self {
            frame,
            token,
            callbacks,
            model,
        } = self;
        // Use each model's checked deletion so adapter registry entries are
        // released with the LVGL object.
        let model = match model {
            SurfaceModel::Home(screen) => screen.destroy(ui_token).map_err(SurfaceModel::Home),
            SurfaceModel::Launcher(screen) => {
                screen.destroy(ui_token).map_err(SurfaceModel::Launcher)
            }
            SurfaceModel::Diagnostics(screen) => {
                screen.destroy(ui_token).map_err(SurfaceModel::Diagnostics)
            }
            SurfaceModel::AmbientView(screen) => {
                screen.destroy(ui_token).map_err(SurfaceModel::AmbientView)
            }
            SurfaceModel::AnalogClock(screen) => {
                screen.destroy(ui_token).map_err(SurfaceModel::AnalogClock)
            }
            SurfaceModel::OverlayToggles(screen) => screen
                .destroy(ui_token)
                .map_err(SurfaceModel::OverlayToggles),
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(screen) => screen
                .destroy(ui_token)
                .map_err(SurfaceModel::ProviderFixture),
        };
        match model {
            Ok(()) => {
                if intent_bridge::release(&callbacks).is_err() {
                    return Err(DestroyFailure::Audit);
                }
                Ok(())
            }
            Err(model) => Err(DestroyFailure::Live(Self {
                frame,
                token,
                callbacks,
                model,
            })),
        }
    }

    pub(super) fn show_gesture(
        &mut self,
        ui_token: &UiAccessToken,
        event: io::LvglGestureEvent,
    ) -> bool {
        match &mut self.model {
            SurfaceModel::Diagnostics(screen) => screen.show_gesture(ui_token, event, true),
            SurfaceModel::Home(_)
            | SurfaceModel::Launcher(_)
            | SurfaceModel::AmbientView(_)
            | SurfaceModel::AnalogClock(_)
            | SurfaceModel::OverlayToggles(_) => false,
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(_) => false,
        }
    }

    fn ambient_view_mut(&mut self) -> Option<&mut ambient_view::AmbientViewScreen> {
        match &mut self.model {
            SurfaceModel::AmbientView(screen) => Some(screen),
            SurfaceModel::Home(_)
            | SurfaceModel::Launcher(_)
            | SurfaceModel::Diagnostics(_)
            | SurfaceModel::AnalogClock(_)
            | SurfaceModel::OverlayToggles(_) => None,
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(_) => None,
        }
    }

    fn analog_clock_mut(&mut self) -> Option<&mut analog_clock::AnalogClockScreen> {
        match &mut self.model {
            SurfaceModel::AnalogClock(screen) => Some(screen),
            SurfaceModel::Home(_)
            | SurfaceModel::Launcher(_)
            | SurfaceModel::Diagnostics(_)
            | SurfaceModel::AmbientView(_)
            | SurfaceModel::OverlayToggles(_) => None,
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(_) => None,
        }
    }
}

struct LvglSurfaceRuntime<'a> {
    surfaces: SurfaceRefs,
    apps: &'a MeditamerApps,
    catalogue: &'a DefaultCatalogue,
    settings: &'a UiSettings,
    ui_token: UiAccessToken,
    transition_started_us: u64,
    clock_shared: &'a mut analog_clock::SharedCache,
    ambient_shared: &'a mut ambient_view::SharedCache,
}

/// Builds an [`LvglSurfaceRuntime`] borrowing only `catalogue`/`settings`, not
/// the whole [`Backend`] -- a `&self`-taking constructor would tie its
/// return's lifetime to all of `self`, which every call site immediately
/// needs alongside a `&mut self.coordinator` borrow for the dispatch the
/// runtime is built for.
fn lvgl_surface_runtime<'a>(
    surfaces: SurfaceRefs,
    apps: &'a MeditamerApps,
    catalogue: &'a DefaultCatalogue,
    settings: &'a UiSettings,
    ui_token: UiAccessToken,
    transition_started_us: u64,
    clock_shared: &'a mut analog_clock::SharedCache,
    ambient_shared: &'a mut ambient_view::SharedCache,
) -> LvglSurfaceRuntime<'a> {
    LvglSurfaceRuntime {
        surfaces,
        apps,
        catalogue,
        settings,
        ui_token,
        transition_started_us,
        clock_shared,
        ambient_shared,
    }
}

impl SurfaceRuntime for LvglSurfaceRuntime<'_> {
    type Instance = ActiveSurface;
    type EnterError = InitError;

    fn enter(
        &mut self,
        frame: NavigationFrame,
        token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError> {
        ActiveSurface::enter(frame, token, self)
    }

    fn activate(&mut self, instance: &Self::Instance) -> bool {
        instance.activate(&self.ui_token)
    }

    fn quiesce(&mut self, instance: &Self::Instance) -> bool {
        instance.disable().is_ok()
    }

    fn enable(&mut self, instance: &Self::Instance) -> bool {
        instance.enable().is_ok()
    }

    fn destroy(&mut self, instance: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        instance.destroy(&self.ui_token)
    }

    fn observe(&mut self, event: LifecycleEvent) {
        if event == LifecycleEvent::CandidateEntered {
            log_lifecycle_resources(
                &self.ui_token,
                "candidate_created",
                Instant::now()
                    .as_micros()
                    .saturating_sub(self.transition_started_us),
            );
        }
    }
}

/// The [`CompositionRuntime`] target for overlays, matching
/// [`LvglSurfaceRuntime`]'s role for screens. `enter`'s overlay kind is
/// inferred purely from `instance.token.surface` identity. This keeps each
/// base/provider overlay's concrete LVGL type aligned with the shell surface
/// admitted for it even though [`CompositionRuntime::enter`] carries no
/// separate product-level overlay kind.
struct LvglOverlayRuntime {
    surfaces: SurfaceRefs,
    input: PointerInputAccess,
    ui_token: UiAccessToken,
    frontlight_calibration_initial: u8,
}

impl CompositionRuntime for LvglOverlayRuntime {
    type Instance = ActiveOverlay;
    type EnterError = OverlayEnterError;

    fn enter(&mut self, instance: OverlayInstance) -> Result<Self::Instance, Self::EnterError> {
        if instance.token.surface == self.surfaces.navigation_cue {
            ActiveOverlay::base(&self.ui_token, instance, BaseOverlayKind::NavigationCue)
        } else if instance.token.surface == self.surfaces.sticky_status {
            ActiveOverlay::base(&self.ui_token, instance, BaseOverlayKind::RefreshControl)
        } else if instance.token.surface == self.surfaces.confirm
            && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::confirm(&self.ui_token, instance)
        } else if instance.token.surface == self.surfaces.settings
            && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::settings(&self.ui_token, instance)
        } else if instance.token.surface == self.surfaces.clock_overlay
            && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::clock(&self.ui_token, instance, self.surfaces.launcher)
        } else if instance.token.surface == self.surfaces.frontlight_calibration
            && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::frontlight_calibration(
                &self.ui_token,
                instance,
                self.frontlight_calibration_initial,
            )
        } else if cfg!(feature = "ui-provider-fixture") && {
            #[cfg(feature = "ui-provider-fixture")]
            {
                instance.token.surface == self.surfaces.provider_overlay
                    && instance.input == OverlayInput::Modal
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                false
            }
        } {
            #[cfg(feature = "ui-provider-fixture")]
            {
                ActiveOverlay::confirm(&self.ui_token, instance)
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                Err(OverlayEnterError::ObjectCreation)
            }
        } else {
            Err(OverlayEnterError::ObjectCreation)
        }
    }

    fn instance(&self, live: &Self::Instance) -> OverlayInstance {
        live.instance()
    }

    fn show(&mut self, live: &Self::Instance) {
        live.show(&self.ui_token);
    }

    fn hide(&mut self, live: &Self::Instance) {
        live.hide(&self.ui_token);
    }

    fn enable(&mut self, live: &Self::Instance) -> bool {
        live.enable().is_ok()
    }

    fn disable(&mut self, live: &Self::Instance) -> bool {
        live.disable().is_ok()
    }

    fn destroy(&mut self, live: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        live.destroy(&self.ui_token)
    }

    fn set_exclusive_capture(&mut self, enabled: bool) {
        // Both LVGL and the acquisition pipeline must forget an in-flight
        // contact, so a surface never inherits the hold that changed capture.
        let _ = self.input.reset(&self.ui_token);
        request_touch_pipeline_reset();
        let _ = self.ui_token.set_system_layer_capture(enabled);
    }
}

pub(crate) struct Backend {
    coordinator: MeditamerCoordinator,
    apps: MeditamerApps,
    catalogue: DefaultCatalogue,
    settings: UiSettingsPersistence,
    surfaces: SurfaceRefs,
    input: PointerInput,
    _display: Display,
    _runtime_session: RuntimeSession,
    // Cloned from the exclusive runtime session for checked widget access.
    ui_token: UiAccessToken,
    timer_metrics: TimerServiceMetrics,
    next_timer_due_ms: u64,
    multitouch: LvglMultitouchTracker,
    environment_reading: Option<EnvironmentSnapshot>,
    environment_state: Option<EnvironmentStateSnapshot>,
    #[cfg(feature = "cpu-load")]
    cpu_reading: Option<cpu_load::Snapshot>,
    /// Set when a Back tap or elapsed timeout requests clock removal but
    /// Clean is not yet confirmed allowed; no widget/coordinator mutation
    /// has happened yet (ADR-0025 item 5).
    clock_removal: ClockRemovalState<SurfaceInstanceToken>,
    pending_input_transition: Option<u32>,
    input_transition_rendered: bool,
    input_transition_readiness: InputTransitionReadiness,
    /// Unpresented Fast clock admission/update work, if any (ADR-0025 item
    /// 5's deferred semantic Fast action).
    pending_fast_clock: PendingFastClock<SurfaceInstanceToken>,
    /// Validated semantic update request waiting to be paired with damage at
    /// the product/presentation boundary.
    pending_screen_updates: [Option<OwnedScreenUpdateRequest>; 2],
    frontlight_calibration_initial: u8,
    pending_frontlight_effect: Option<FrontlightCalibrationEffect>,
    /// Boot-lifetime analog-clock resources (validated asset bytes, publish
    /// canvas, stationary base cache). Small holder only; per-activation
    /// workspace lives in the screen and is released on exit.
    analog_clock_shared: analog_clock::SharedCache,
    /// Boot-lifetime Ambient Home sky/sun resources (validated pack bytes,
    /// publish canvas, composition cursor). Same shape as above.
    ambient_shared: ambient_view::SharedCache,
    #[cfg(feature = "ui-provider-fixture")]
    provider_fixture_state: ProviderFixtureState,
}

impl Backend {
    fn overlay_runtime(&self) -> LvglOverlayRuntime {
        LvglOverlayRuntime {
            surfaces: self.surfaces,
            input: self.input.access(),
            ui_token: self.ui_token.clone(),
            frontlight_calibration_initial: self.frontlight_calibration_initial,
        }
    }

    /// Only the committed, renderable Ambient Home instance owns environment
    /// delivery. Staged candidates and failed transitions never acquire it.
    pub(crate) fn committed_environment_owner(&self) -> Option<SurfaceInstanceToken> {
        let active = self.coordinator.active_screen()?;
        (matches!(active.model, SurfaceModel::AmbientView(_))
            && self.active_surface_is_renderable())
        .then_some(active.token)
    }

    pub(crate) fn take_due_settings_write(&mut self, now_ms: u64) -> Option<PersistedUiSettings> {
        self.settings.take_due(now_ms)
    }

    pub(crate) fn complete_settings_write(&mut self, success: bool, now_ms: u64) {
        self.settings.complete(success, now_ms);
    }

    pub(crate) fn active_surface_label(&self) -> Option<&'static str> {
        let surface = self.coordinator.shell().active().surface;
        if surface == self.surfaces.home {
            Some("home")
        } else if surface == self.surfaces.launcher {
            Some("launcher")
        } else if surface == self.surfaces.diagnostics {
            Some("diagnostics")
        } else if surface == self.surfaces.ambient_view {
            Some("ambient_view")
        } else if surface == self.surfaces.analog_clock {
            Some("analog_clock")
        } else if surface == self.surfaces.overlay_toggles {
            Some("overlay_toggles")
        } else if cfg!(feature = "ui-provider-fixture") && {
            #[cfg(feature = "ui-provider-fixture")]
            {
                surface == self.surfaces.provider_fixture
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                false
            }
        } {
            Some("provider_fixture")
        } else {
            None
        }
    }

    pub(super) fn active_surface_is_renderable(&self) -> bool {
        self.coordinator.active_screen().is_some_and(|instance| {
            instance
                .root_widget()
                .is_active_screen(&self.ui_token)
                .unwrap_or(false)
                && instance.frame == self.coordinator.shell().active()
                && instance.token == self.coordinator.shell().active_instance()
        })
    }

    pub(super) fn log_lifecycle_checkpoint(&self, phase: &str, transition_us: u64) {
        observability::record_stack_headroom();
        let monitor = self
            .ui_token
            .memory_snapshot()
            .expect("live LVGL runtime session");
        let allocator = psram::allocator_memory_snapshot();
        let active = self
            .coordinator
            .active_screen()
            .map(|instance| instance.token);
        let shell_aligned = self.coordinator.active_screen().is_some_and(|instance| {
            instance.frame == self.coordinator.shell().active()
                && instance.token == self.coordinator.shell().active_instance()
        });
        console::println!(
            "LVGL_LIFECYCLE phase={} active={:?} shell_aligned={} transition_us={} lvgl_total={} lvgl_used={} lvgl_free={} lvgl_biggest_free={} lvgl_used_blocks={} lvgl_free_blocks={} lvgl_max_used={} lvgl_frag_pct={} integrity_ok={} heap_internal_free={} heap_internal_min={} heap_external_free={} heap_external_min={} heap_peak_used={} cpu0_stack_min={} timer_gap_max_us={} timer_runtime_max_us={} cleanup_blocked={} navigation_faulted={} composition_faulted={} lifecycle_audit_faulted={}",
            phase,
            active,
            shell_aligned,
            transition_us,
            monitor.total_size,
            monitor.total_size.saturating_sub(monitor.free_size),
            monitor.free_size,
            monitor.largest_free_size,
            monitor.used_count,
            monitor.free_count,
            monitor.max_used,
            monitor.fragmentation_percent,
            monitor.integrity_ok,
            allocator.free_internal_bytes,
            allocator.min_free_internal_bytes,
            allocator.free_external_bytes,
            allocator.min_free_external_bytes,
            allocator.peak_used_bytes,
            observability::minimum_stack_headroom_bytes(),
            self.timer_metrics.max_gap_us(),
            self.timer_metrics.max_runtime_us(),
            self.coordinator.cleanup_blocked_screen().is_some()
                || self.coordinator.overlay_cleanup_blocked_len() != 0,
            self.coordinator.navigation_faulted(),
            self.coordinator.composition_faulted(),
            self.coordinator.lifecycle_audit_faulted(),
        );
    }
}

fn destroy_initial_surface_or_stop(surface: ActiveSurface, ui_token: &UiAccessToken) {
    match surface.destroy(ui_token) {
        Ok(()) => {}
        Err(DestroyFailure::Live(_)) => {
            panic!("initial LVGL surface remained live after cleanup")
        }
        Err(DestroyFailure::Audit) => {
            panic!("initial LVGL callback route audit failed after cleanup")
        }
    }
}

fn destroy_initial_overlay_or_stop(overlay: ActiveOverlay, ui_token: &UiAccessToken) {
    match overlay.destroy(ui_token) {
        Ok(()) => {}
        Err(DestroyFailure::Live(_)) => {
            panic!("initial LVGL overlay remained live after cleanup")
        }
        Err(DestroyFailure::Audit) => {
            panic!("initial LVGL overlay reported an impossible audit failure")
        }
    }
}

fn destroy_initial_backend_or_stop(backend: Backend) {
    let ui_token = backend.ui_token.clone();
    let (active, cleanup_blocked, overlays, overlay_cleanup_blocked) =
        backend.coordinator.into_owned_instances();
    for overlay in overlay_cleanup_blocked {
        destroy_initial_overlay_or_stop(overlay, &ui_token);
    }
    for overlay in overlays {
        destroy_initial_overlay_or_stop(overlay, &ui_token);
    }
    if let Some(surface) = cleanup_blocked {
        destroy_initial_surface_or_stop(surface, &ui_token);
    }
    if let Some(surface) = active {
        destroy_initial_surface_or_stop(surface, &ui_token);
    }
}

fn log_lifecycle_resources(ui_token: &UiAccessToken, phase: &str, transition_us: u64) {
    observability::record_stack_headroom();
    let monitor = ui_token
        .memory_snapshot()
        .expect("live LVGL runtime session");
    let allocator = psram::allocator_memory_snapshot();
    console::println!(
        "LVGL_LIFECYCLE phase={} transition_us={} lvgl_total={} lvgl_used={} lvgl_free={} lvgl_biggest_free={} lvgl_used_blocks={} lvgl_free_blocks={} lvgl_max_used={} lvgl_frag_pct={} integrity_ok={} heap_internal_free={} heap_internal_min={} heap_external_free={} heap_external_min={} heap_peak_used={} cpu0_stack_min={}",
        phase,
        transition_us,
        monitor.total_size,
        monitor.total_size.saturating_sub(monitor.free_size),
        monitor.free_size,
        monitor.largest_free_size,
        monitor.used_count,
        monitor.free_count,
        monitor.max_used,
        monitor.fragmentation_percent,
        monitor.integrity_ok,
        allocator.free_internal_bytes,
        allocator.min_free_internal_bytes,
        allocator.free_external_bytes,
        allocator.min_free_external_bytes,
        allocator.peak_used_bytes,
        observability::minimum_stack_headroom_bytes(),
    );
}

fn prepare_memory_pool() -> Result<(), InitError> {
    if !MEMORY_POOL.load(Ordering::Acquire).is_null() {
        return Ok(());
    }
    let Ok(mut pool) = psram::alloc_large_byte_buffer(MEMORY_POOL_BYTES) else {
        return Err(InitError::MemoryPoolUnavailable);
    };
    if pool.placement() != BufferPlacement::Psram {
        return Err(InitError::MemoryPoolUnavailable);
    }
    let pool_ptr = pool.as_mut_slice().as_mut_ptr();
    if !(pool_ptr as usize).is_multiple_of(core::mem::align_of::<u32>()) {
        return Err(InitError::MemoryPoolUnavailable);
    }
    MEMORY_POOL.store(pool_ptr, Ordering::Release);
    core::mem::forget(pool);
    psram::log_allocator_high_water("lvgl_memory_pool_alloc");
    Ok(())
}

pub(super) fn alloc_pool(size: usize) -> *mut core::ffi::c_void {
    if size > MEMORY_POOL_BYTES {
        return ptr::null_mut();
    }
    MEMORY_POOL.load(Ordering::Acquire).cast()
}
