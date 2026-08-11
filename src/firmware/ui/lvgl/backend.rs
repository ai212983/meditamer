use core::sync::atomic::{AtomicPtr, Ordering};
use core::{mem::MaybeUninit, ptr};

use embassy_time::Instant;
use heapless::Vec;
use lightvgl_sys as lv;

use super::base_overlays::{ActiveOverlay, BaseOverlayKind, OverlayEnterError};
use super::dither::DirtyArea;
#[cfg(feature = "ui-provider-fixture")]
use super::provider_fixture;
use super::{gesture_test, home, intent_bridge, io, launcher, HEIGHT, WIDTH};
#[cfg(feature = "ui-provider-fixture")]
use crate::firmware::ui::shell::model::{PendingProviderRemoval, ProviderRuntimeAudit};
use crate::firmware::{
    psram::{self, BufferPlacement},
    telemetry,
    touch::lvgl_multitouch::{LvglContactBatch, LvglMultitouchFrame, LvglMultitouchTracker},
    touch::types::TouchEvent,
    types::InkplateDriver,
    ui::shell::{
        composition::CompositionPlanResult,
        lifecycle::{
            execute_transition, DestroyFailure, LifecycleEvent, RollbackReason, SurfaceRuntime,
            TransitionResult,
        },
        model::{
            DefaultShellModel, PreparedComposition, LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY,
        },
        navigator::NavigationFrame,
        timing::TimerServiceMetrics,
        types::{
            CompositionIntent, NavIntent, OverlayAdmission, OverlayDismissal, OverlayInput,
            OverlayInstance, OverlayLifetime, OwnedCompositionIntent, OwnedNavIntent,
            OwnedShellIntent, ProviderId, RefreshHint, RefreshIntent, SurfaceCapabilities,
            SurfaceId, SurfaceInstanceToken, SurfaceRef, SurfaceRole, SurfaceSpec,
        },
    },
};

const BUFFER_LINES: usize = 16;
const BUFFER_BYTES: usize = WIDTH as usize * BUFFER_LINES;
const BUFFER_WORDS: usize = BUFFER_BYTES.div_ceil(core::mem::size_of::<u32>());
const MEMORY_POOL_BYTES: usize = 128 * 1024;
const BASE_PROVIDER_ID: ProviderId = ProviderId(1);
const HOME_SURFACE_ID: SurfaceId = SurfaceId(1);
const LAUNCHER_SURFACE_ID: SurfaceId = SurfaceId(2);
const DIAGNOSTICS_SURFACE_ID: SurfaceId = SurfaceId(3);
const NAVIGATION_CUE_SURFACE_ID: SurfaceId = SurfaceId(4);
const STICKY_STATUS_SURFACE_ID: SurfaceId = SurfaceId(5);
const CONFIRM_SURFACE_ID: SurfaceId = SurfaceId(6);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ID: ProviderId = ProviderId(2);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ROOT_ID: SurfaceId = SurfaceId(101);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_OVERLAY_ID: SurfaceId = SurfaceId(102);
const BASE_SURFACES: [SurfaceSpec; 6] = [
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
    SurfaceSpec::new(
        DIAGNOSTICS_SURFACE_ID.0,
        SurfaceRole::SystemRoot,
        SurfaceCapabilities::LAUNCHABLE,
        RefreshHint::Boundary,
    ),
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
];
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

static LVGL_DISPLAY: AtomicPtr<lv::lv_display_t> = AtomicPtr::new(ptr::null_mut());
static LVGL_INPUT: AtomicPtr<lv::lv_indev_t> = AtomicPtr::new(ptr::null_mut());
static MEMORY_POOL: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static mut DRAW_BUFFER: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitError {
    MemoryPoolUnavailable,
    DisplayCreationFailed,
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
    navigation_cue: SurfaceRef,
    sticky_status: SurfaceRef,
    confirm: SurfaceRef,
    #[cfg(feature = "ui-provider-fixture")]
    provider_fixture: SurfaceRef,
    #[cfg(feature = "ui-provider-fixture")]
    provider_overlay: SurfaceRef,
}

enum SurfaceModel {
    Home(home::HomeScreen),
    Launcher(launcher::LauncherScreen),
    Diagnostics(gesture_test::GestureTestScreen),
    #[cfg(feature = "ui-provider-fixture")]
    ProviderFixture(provider_fixture::ProviderFixtureScreen),
}

#[cfg(feature = "ui-provider-fixture")]
enum ProviderFixtureState {
    Registered(crate::firmware::ui::shell::types::ProviderToken),
    Detaching(PendingProviderRemoval),
    Removed,
}

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
        surfaces: SurfaceRefs,
    ) -> Result<Self, InitError> {
        #[cfg(feature = "ui-provider-fixture")]
        let requested_overlay = if frame.surface == surfaces.provider_fixture {
            surfaces.provider_overlay
        } else {
            surfaces.confirm
        };
        #[cfg(not(feature = "ui-provider-fixture"))]
        let requested_overlay = surfaces.confirm;
        let bindings = intent_bridge::IntentBindings::Screen {
            open_launcher: OwnedNavIntent {
                source: token,
                intent: NavIntent::OpenLauncher(surfaces.launcher),
            },
            launch_diagnostics: OwnedNavIntent {
                source: token,
                intent: NavIntent::Launch(surfaces.diagnostics),
            },
            home: OwnedNavIntent {
                source: token,
                intent: NavIntent::Home,
            },
            show_confirm: OwnedCompositionIntent {
                source: token,
                intent: CompositionIntent::Request {
                    surface: requested_overlay,
                    input: OverlayInput::Modal,
                    lifetime: if requested_overlay == surfaces.confirm {
                        OverlayLifetime::Transient
                    } else {
                        OverlayLifetime::Sticky
                    },
                    rank: 3,
                },
            },
        };
        let callbacks =
            intent_bridge::claim(bindings).map_err(|_| InitError::CallbackRouteUnavailable)?;
        let user_data = callbacks.user_data();
        let model = unsafe {
            if frame.surface == surfaces.home {
                home::create(user_data).map(SurfaceModel::Home)
            } else if frame.surface == surfaces.launcher {
                launcher::create(user_data).map(SurfaceModel::Launcher)
            } else if frame.surface == surfaces.diagnostics {
                gesture_test::create(user_data).map(SurfaceModel::Diagnostics)
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
                    provider_fixture::create(user_data).map(SurfaceModel::ProviderFixture)
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

    fn root(&self) -> *mut lv::lv_obj_t {
        match &self.model {
            SurfaceModel::Home(screen) => screen.root(),
            SurfaceModel::Launcher(screen) => screen.root(),
            SurfaceModel::Diagnostics(screen) => screen.root(),
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(screen) => screen.root(),
        }
    }

    fn activate(&self) -> bool {
        activate_root(self.root())
    }

    fn enable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::enable(&self.callbacks)
    }

    fn disable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::disable(&self.callbacks)
    }

    fn destroy(self) -> Result<(), DestroyFailure<Self>> {
        if self.disable().is_err() {
            return Err(DestroyFailure::Live(self));
        }
        intent_bridge::purge_instance(self.token);
        let root = self.root();
        unsafe { lv::lv_obj_delete(root) };
        if unsafe { lv::lv_obj_is_valid(root) } {
            return Err(DestroyFailure::Live(self));
        }
        if intent_bridge::release(&self.callbacks).is_err() {
            return Err(DestroyFailure::Audit);
        }
        Ok(())
    }

    fn show_gesture(&mut self, event: io::LvglGestureEvent) -> bool {
        match &mut self.model {
            SurfaceModel::Diagnostics(screen) => unsafe { screen.show_gesture(event, true) },
            SurfaceModel::Home(_) | SurfaceModel::Launcher(_) => false,
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(_) => false,
        }
    }
}

struct LvglSurfaceRuntime {
    surfaces: SurfaceRefs,
    transition_started_us: u64,
}

impl SurfaceRuntime for LvglSurfaceRuntime {
    type Instance = ActiveSurface;
    type EnterError = InitError;

    fn enter(
        &mut self,
        frame: NavigationFrame,
        token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError> {
        ActiveSurface::enter(frame, token, self.surfaces)
    }

    fn activate(&mut self, instance: &Self::Instance) -> bool {
        instance.activate()
    }

    fn quiesce(&mut self, instance: &Self::Instance) -> bool {
        instance.disable().is_ok()
    }

    fn enable(&mut self, instance: &Self::Instance) -> bool {
        instance.enable().is_ok()
    }

    fn destroy(&mut self, instance: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>> {
        instance.destroy()
    }

    fn observe(&mut self, event: LifecycleEvent) {
        if event == LifecycleEvent::CandidateEntered {
            log_lifecycle_resources(
                "candidate_created",
                Instant::now()
                    .as_micros()
                    .saturating_sub(self.transition_started_us),
            );
        }
    }
}

pub(crate) struct Backend {
    shell: DefaultShellModel,
    surfaces: SurfaceRefs,
    active: Option<ActiveSurface>,
    cleanup_blocked: Option<ActiveSurface>,
    overlays: Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    overlay_cleanup_blocked: Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    composition_faulted: bool,
    lifecycle_audit_faulted: bool,
    navigation_faulted: bool,
    timer_metrics: TimerServiceMetrics,
    multitouch: LvglMultitouchTracker,
    #[cfg(feature = "ui-provider-fixture")]
    provider_fixture_state: ProviderFixtureState,
}

impl Backend {
    pub(crate) fn initialize(
        display: &mut InkplateDriver,
    ) -> Result<(Self, Option<DirtyArea>), InitError> {
        let shell = DefaultShellModel::new(BASE_PROVIDER_ID, &BASE_SURFACES, HOME_SURFACE_ID)
            .map_err(|_| InitError::ShellConfigurationFailed)?;
        #[cfg(feature = "ui-provider-fixture")]
        let mut shell = shell;
        #[cfg(feature = "ui-provider-fixture")]
        let provider_fixture_owner = shell
            .register_provider(PROVIDER_FIXTURE_ID, &PROVIDER_FIXTURE_SURFACES)
            .map_err(|_| InitError::ShellConfigurationFailed)?;
        let base_owner = shell.active().surface.owner;
        let home_surface = SurfaceRef::new(base_owner, HOME_SURFACE_ID.0);
        let launcher_surface = SurfaceRef::new(base_owner, LAUNCHER_SURFACE_ID.0);
        let diagnostics_surface = SurfaceRef::new(base_owner, DIAGNOSTICS_SURFACE_ID.0);
        let surfaces = SurfaceRefs {
            home: home_surface,
            launcher: launcher_surface,
            diagnostics: diagnostics_surface,
            navigation_cue: SurfaceRef::new(base_owner, NAVIGATION_CUE_SURFACE_ID.0),
            sticky_status: SurfaceRef::new(base_owner, STICKY_STATUS_SURFACE_ID.0),
            confirm: SurfaceRef::new(base_owner, CONFIRM_SURFACE_ID.0),
            #[cfg(feature = "ui-provider-fixture")]
            provider_fixture: SurfaceRef::new(provider_fixture_owner, PROVIDER_FIXTURE_ROOT_ID.0),
            #[cfg(feature = "ui-provider-fixture")]
            provider_overlay: SurfaceRef::new(
                provider_fixture_owner,
                PROVIDER_FIXTURE_OVERLAY_ID.0,
            ),
        };

        prepare_memory_pool()?;

        let bootstrap_root = unsafe {
            lv::lv_init();
            let lv_display = lv::lv_display_create(WIDTH, HEIGHT);
            if lv_display.is_null() {
                return Err(InitError::DisplayCreationFailed);
            }
            LVGL_DISPLAY.store(lv_display, Ordering::Release);
            lv::lv_display_set_color_format(lv_display, lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
            lv::lv_display_set_buffers(
                lv_display,
                ptr::addr_of_mut!(DRAW_BUFFER).cast(),
                ptr::null_mut(),
                BUFFER_BYTES as u32,
                lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
            );
            lv::lv_display_set_flush_cb(lv_display, Some(io::flush_callback));

            let input = lv::lv_indev_create();
            if !input.is_null() {
                lv::lv_indev_set_type(input, lv::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
                lv::lv_indev_set_read_cb(input, Some(io::input_callback));
                lv::lv_indev_set_rotation_rad_threshold(input, ROTATION_THRESHOLD_RADIANS);
                io::register_gesture_input(input);
                lv::lv_indev_add_event_cb(
                    input,
                    Some(io::gesture_callback),
                    lv::lv_event_code_t_LV_EVENT_GESTURE,
                    ptr::null_mut(),
                );
                LVGL_INPUT.store(input, Ordering::Release);
            }

            lv::lv_screen_active()
        };
        if bootstrap_root.is_null() {
            return Err(InitError::SurfaceCreationFailed);
        }
        let home = ActiveSurface::enter(shell.active(), shell.active_instance(), surfaces)?;
        if !home.activate() {
            destroy_initial_surface_or_stop(home);
            return Err(InitError::SurfaceActivationFailed);
        }
        if home.enable().is_err() {
            let _ = activate_root(bootstrap_root);
            destroy_initial_surface_or_stop(home);
            return Err(InitError::CallbackRouteUnavailable);
        }
        if unsafe { bootstrap_root != home.root() && lv::lv_obj_is_valid(bootstrap_root) } {
            unsafe { lv::lv_obj_delete(bootstrap_root) };
            if unsafe { lv::lv_obj_is_valid(bootstrap_root) } {
                let _ = activate_root(bootstrap_root);
                destroy_initial_surface_or_stop(home);
                return Err(InitError::SurfaceCleanupFailed);
            }
        }

        let now_us = Instant::now().as_micros();
        let mut backend = Self {
            shell,
            surfaces,
            active: Some(home),
            cleanup_blocked: None,
            overlays: Vec::new(),
            overlay_cleanup_blocked: Vec::new(),
            composition_faulted: false,
            lifecycle_audit_faulted: false,
            navigation_faulted: false,
            timer_metrics: TimerServiceMetrics::new(now_us),
            multitouch: LvglMultitouchTracker::default(),
            #[cfg(feature = "ui-provider-fixture")]
            provider_fixture_state: ProviderFixtureState::Registered(provider_fixture_owner),
        };
        if let Err(error) = backend.install_base_overlay(
            surfaces.navigation_cue,
            OverlayLifetime::Transient,
            1,
            BaseOverlayKind::NavigationCue,
        ) {
            destroy_initial_backend_or_stop(backend);
            return Err(error);
        }
        if let Err(error) = backend.install_base_overlay(
            surfaces.sticky_status,
            OverlayLifetime::Sticky,
            2,
            BaseOverlayKind::RefreshControl,
        ) {
            destroy_initial_backend_or_stop(backend);
            return Err(error);
        }
        backend.log_lifecycle_checkpoint("initialized", 0);
        let rendered = backend
            .run_timers(display, 250)
            .or_else(|| backend.invalidate(display));
        Ok((backend, rendered))
    }

    fn install_base_overlay(
        &mut self,
        surface: SurfaceRef,
        lifetime: OverlayLifetime,
        rank: u8,
        kind: BaseOverlayKind,
    ) -> Result<(), InitError> {
        let prepared = self
            .shell
            .prepare_overlay_request(surface, kind.input(), lifetime, rank)
            .map_err(|_| InitError::ShellConfigurationFailed)?;
        let instance = prepared
            .delta()
            .enter_live()
            .first()
            .copied()
            .ok_or(InitError::ShellConfigurationFailed)?;
        let candidate =
            ActiveOverlay::base(instance, kind).map_err(|_| InitError::SurfaceCreationFailed)?;
        if let Err(candidate) = self.overlays.push(candidate) {
            destroy_initial_overlay_or_stop(candidate);
            return Err(InitError::ShellConfigurationFailed);
        }
        match self.shell.commit_overlay_request(prepared) {
            Ok(OverlayAdmission::Active(committed)) if committed == instance => {
                self.overlays
                    .last()
                    .expect("the committed overlay remains runtime-owned")
                    .show();
                Ok(())
            }
            Ok(OverlayAdmission::Active(_) | OverlayAdmission::Queued(_)) | Err(_) => {
                let candidate = self
                    .overlays
                    .pop()
                    .expect("the uncommitted overlay remains runtime-owned");
                destroy_initial_overlay_or_stop(candidate);
                Err(InitError::ShellConfigurationFailed)
            }
        }
    }

    pub(crate) fn handle_touch(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> Option<DirtyArea> {
        io::update_touch(event);
        self.render_with(display, |_| unsafe {
            let input = LVGL_INPUT.load(Ordering::Acquire);
            if !input.is_null() {
                // Force one read per pipeline event so a queued Down+Up pair cannot
                // collapse into a single released sample before LVGL observes it.
                lv::lv_indev_read(input);
            }
        })
    }

    pub(crate) fn handle_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        frame: LvglMultitouchFrame,
    ) -> Option<DirtyArea> {
        let (batch, terminating) = self.multitouch.update_gesture(frame);
        self.read_multitouch(display, batch, terminating)
    }

    pub(crate) fn reset_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        t_ms: u64,
    ) -> Option<DirtyArea> {
        let releases = self.multitouch.release_all(t_ms);
        if releases.is_empty() {
            self.multitouch.reset();
            return None;
        }
        let rendered = self.read_multitouch(display, releases, true);
        self.multitouch.reset();
        rendered
    }

    pub(crate) fn show_gesture(
        &mut self,
        display: &mut InkplateDriver,
        event: io::LvglGestureEvent,
    ) -> Option<DirtyArea> {
        if self.shell.active_modal().is_some() {
            esp_println::println!("UI_GESTURE state=blocked reason=modal_active event={event:?}");
            return None;
        }
        self.render_with(display, |backend| {
            if let Some(active) = backend.active.as_mut() {
                let _ = active.show_gesture(event);
            }
        })
    }

    pub(crate) fn run_timers(
        &mut self,
        display: &mut InkplateDriver,
        elapsed_ms: u32,
    ) -> Option<DirtyArea> {
        self.render_with(display, |backend| unsafe {
            let started_us = Instant::now().as_micros();
            backend.timer_metrics.begin_handler(started_us);
            lv::lv_tick_inc(elapsed_ms);
            lv::lv_timer_handler();
            let runtime_us = Instant::now().as_micros().saturating_sub(started_us);
            backend.timer_metrics.finish_handler(runtime_us);
        })
    }

    pub(crate) fn invalidate(&mut self, display: &mut InkplateDriver) -> Option<DirtyArea> {
        self.render_with(display, |_| unsafe {
            let screen = lv::lv_screen_active();
            if !screen.is_null() {
                let _ = lv::lv_obj_invalidate(screen);
            }
        })
    }

    pub(crate) fn cycle_step(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Result<DirtyArea, UiCycleStepError> {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        if self.shell.active_modal().is_some() {
            return Err(UiCycleStepError::Busy);
        }
        if !self.active_surface_is_renderable() {
            return Err(UiCycleStepError::NavigationFault);
        }

        let current = self.shell.active().surface;
        let (intent, expected) = if current == self.surfaces.home {
            (
                NavIntent::OpenLauncher(self.surfaces.launcher),
                self.surfaces.launcher,
            )
        } else if current == self.surfaces.launcher {
            (
                NavIntent::Launch(self.surfaces.diagnostics),
                self.surfaces.diagnostics,
            )
        } else if current == self.surfaces.diagnostics {
            (NavIntent::Home, self.surfaces.home)
        } else {
            return Err(UiCycleStepError::NavigationFault);
        };

        self.shell
            .queue_intent(OwnedNavIntent {
                source: self.shell.active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)?;
        let dirty = self.render_with(display, |_| {});
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || self.shell.active().surface != expected
            || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        dirty.ok_or(UiCycleStepError::NoDirty)
    }

    #[cfg(feature = "ui-provider-fixture")]
    pub(crate) fn provider_fixture_step(
        &mut self,
        display: &mut InkplateDriver,
    ) -> Result<DirtyArea, UiCycleStepError> {
        if self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || matches!(
                self.provider_fixture_state,
                ProviderFixtureState::Detaching(_)
            )
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        if !self.active_surface_is_renderable() {
            return Err(UiCycleStepError::NavigationFault);
        }

        if matches!(self.provider_fixture_state, ProviderFixtureState::Removed) {
            if self.shell.active().surface != self.surfaces.home {
                return Err(UiCycleStepError::NavigationFault);
            }
            let owner = self
                .shell
                .register_provider(PROVIDER_FIXTURE_ID, &PROVIDER_FIXTURE_SURFACES)
                .map_err(|_| UiCycleStepError::NavigationFault)?;
            self.surfaces.provider_fixture = SurfaceRef::new(owner, PROVIDER_FIXTURE_ROOT_ID.0);
            self.surfaces.provider_overlay = SurfaceRef::new(owner, PROVIDER_FIXTURE_OVERLAY_ID.0);
            self.provider_fixture_state = ProviderFixtureState::Registered(owner);
        }

        let current = self.shell.active().surface;
        let dirty = if current == self.surfaces.home {
            self.queue_fixture_navigation(NavIntent::OpenLauncher(self.surfaces.launcher))?;
            self.render_with(display, |_| {})
        } else if current == self.surfaces.launcher {
            self.queue_fixture_navigation(NavIntent::Launch(self.surfaces.provider_fixture))?;
            self.render_with(display, |_| {})
        } else if current == self.surfaces.provider_fixture {
            match self.shell.active_modal() {
                None => {
                    let owned = self.fixture_modal_request(self.surfaces.provider_overlay, 1);
                    self.render_with(display, |backend| backend.drain_composition_intent(owned))
                }
                Some(active) if active.token.surface == self.surfaces.provider_overlay => {
                    // A protected base modal replaces the live provider modal. The
                    // request remains owned by this provider generation.
                    let owned = self.fixture_modal_request(self.surfaces.confirm, 4);
                    self.render_with(display, |backend| backend.drain_composition_intent(owned))
                }
                Some(active) if active.token.surface == self.surfaces.confirm => {
                    let ProviderFixtureState::Registered(owner) = self.provider_fixture_state
                    else {
                        return Err(UiCycleStepError::NavigationFault);
                    };
                    if active.request_owner != owner || self.shell.queued_modal_len() != 0 {
                        return Err(UiCycleStepError::NavigationFault);
                    }
                    let owned = self.fixture_modal_request(self.surfaces.provider_overlay, 1);
                    self.render_with(display, move |backend| {
                        backend.drain_composition_intent(owned);
                        let queued = backend.shell.queued_modal(0);
                        if backend.shell.queued_modal_len() != 1
                            || queued.is_none_or(|queued| {
                                queued.token.surface != backend.surfaces.provider_overlay
                                    || queued.request_owner != owner
                            })
                        {
                            backend.navigation_faulted = true;
                            backend.composition_faulted = true;
                            esp_println::println!(
                                "UI_PROVIDER_REMOVE state=fault stage=queued_owner owner={:?}",
                                owner,
                            );
                            return;
                        }
                        esp_println::println!(
                            "UI_PROVIDER_REMOVE state=staged owner={:?} provider_requested_base_live=true provider_modal_queued=true",
                            owner,
                        );
                        if intent_bridge::queued_provider_action_count(owner) != 0
                            || !backend.send_provider_remove_clicked()
                            || intent_bridge::queued_provider_action_count(owner) != 1
                        {
                            intent_bridge::purge_provider(owner);
                            backend.navigation_faulted = true;
                            backend.composition_faulted = true;
                            esp_println::println!(
                                "UI_PROVIDER_REMOVE state=fault stage=callback_probe owner={:?}",
                                owner,
                            );
                            return;
                        }
                        backend.execute_provider_removal_fixture();
                    })
                }
                Some(_) => return Err(UiCycleStepError::NavigationFault),
            }
        } else {
            return Err(UiCycleStepError::NavigationFault);
        };

        if self.navigation_faulted
            || self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || !self.active_surface_is_renderable()
        {
            return Err(UiCycleStepError::NavigationFault);
        }
        dirty.ok_or(UiCycleStepError::NoDirty)
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn queue_fixture_navigation(&mut self, intent: NavIntent) -> Result<(), UiCycleStepError> {
        self.shell
            .queue_intent(OwnedNavIntent {
                source: self.shell.active_instance(),
                intent,
            })
            .map_err(|_| UiCycleStepError::Busy)
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn fixture_modal_request(&self, surface: SurfaceRef, rank: u8) -> OwnedCompositionIntent {
        OwnedCompositionIntent {
            source: self.shell.active_instance(),
            intent: CompositionIntent::Request {
                surface,
                input: OverlayInput::Modal,
                lifetime: OverlayLifetime::Sticky,
                rank,
            },
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn send_provider_remove_clicked(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            matches!(
                &active.model,
                SurfaceModel::ProviderFixture(screen) if screen.send_remove_clicked()
            )
        })
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn execute_provider_removal_fixture(&mut self) {
        let ProviderFixtureState::Registered(owner) = self.provider_fixture_state else {
            self.navigation_faulted = true;
            return;
        };
        if self.navigation_faulted
            || self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            self.navigation_faulted = true;
            return;
        }
        let plan = match self.shell.prepare_provider_removal(owner) {
            Ok(plan) => plan,
            Err(error) => {
                self.navigation_faulted = true;
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=rejected stage=prepare owner={:?} error={:?}",
                    owner,
                    error,
                );
                return;
            }
        };
        let Some(fallback) = plan.fallback_transition() else {
            self.navigation_faulted = true;
            esp_println::println!(
                "UI_PROVIDER_REMOVE state=rejected stage=fallback owner={:?}",
                owner,
            );
            return;
        };
        if self.active.as_ref().is_none_or(|active| {
            active.frame != fallback.origin
                || active.token != self.shell.active_instance()
                || active.token.surface.owner != owner
        }) {
            self.navigation_faulted = true;
            esp_println::println!(
                "UI_PROVIDER_REMOVE state=fault stage=alignment owner={:?}",
                owner,
            );
            return;
        }
        let mut entering_overlays =
            match self.stage_overlay_entries(plan.composition_delta().enter_live()) {
                Some(candidates) => candidates,
                None => {
                    esp_println::println!(
                        "UI_PROVIDER_REMOVE state=rolled_back stage=overlay_entry owner={:?}",
                        owner,
                    );
                    return;
                }
            };
        let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for instance in plan.composition_delta().leave_live() {
            departing_overlays
                .push(instance.token)
                .expect("the provider delta is bounded by live overlay capacity");
        }
        if !self.stage_overlay_departures(&departing_overlays) {
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            esp_println::println!(
                "UI_PROVIDER_REMOVE state=fault stage=overlay_alignment owner={:?}",
                owner,
            );
            return;
        }
        let Some(origin) = self.active.take() else {
            self.restore_overlay_departures(&departing_overlays);
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.navigation_faulted = true;
            return;
        };
        let transition_started_us = Instant::now().as_micros();
        let mut runtime = LvglSurfaceRuntime {
            surfaces: self.surfaces,
            transition_started_us,
        };
        let result = execute_transition(
            &mut runtime,
            origin,
            fallback.destination,
            fallback.destination_instance,
            || {
                let pending = self.shell.commit_provider_detach(plan)?;
                let purged = intent_bridge::purge_provider(owner);
                if purged != 1 {
                    self.navigation_faulted = true;
                    self.lifecycle_audit_faulted = true;
                    esp_println::println!(
                        "UI_PROVIDER_REMOVE state=audit_failed stage=callback_purge owner={:?} expected=1 actual={}",
                        owner,
                        purged,
                    );
                }
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=detached owner={:?} callback_actions_purged={}",
                    owner,
                    purged,
                );
                Ok::<_, crate::firmware::ui::shell::model::ProviderRemovalError>(pending)
            },
        );
        match result {
            TransitionResult::Committed {
                active,
                outcome: pending,
            } => {
                self.active = Some(active);
                self.provider_fixture_state = ProviderFixtureState::Detaching(pending);
                self.clear_provider_fixture_refs();
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.try_finalize_provider_fixture_removal();
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = self.cleanup_blocked.is_some()
                    || cleanup_audit_failed
                    || !self.active_surface_is_renderable();
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=rolled_back owner={:?} reason={:?}",
                    owner,
                    reason,
                );
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                outcome: pending,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                self.provider_fixture_state = ProviderFixtureState::Detaching(pending);
                self.clear_provider_fixture_refs();
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.navigation_faulted = true;
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=cleanup_blocked owner={:?} reason={:?}",
                    owner,
                    reason,
                );
            }
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn try_finalize_provider_fixture_removal(&mut self) {
        let ProviderFixtureState::Detaching(pending) = &self.provider_fixture_state else {
            return;
        };
        let owner = pending.owner();
        if self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
            || self.composition_faulted
            || self.lifecycle_audit_faulted
        {
            return;
        }
        let runtime_references = self
            .active
            .as_ref()
            .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .cleanup_blocked
                .as_ref()
                .is_some_and(|active| active.token.surface.owner == owner)
            || self
                .overlays
                .iter()
                .chain(self.overlay_cleanup_blocked.iter())
                .any(|overlay| overlay.references_provider(owner));
        let callback_references = intent_bridge::references_provider(owner);
        let integrity_ok = unsafe { lv::lv_mem_test() == lv::lv_result_t_LV_RESULT_OK };
        let shell_aligned = self.active_surface_is_renderable();
        if runtime_references || callback_references || !integrity_ok || !shell_aligned {
            self.navigation_faulted = true;
            self.lifecycle_audit_faulted = true;
            esp_println::println!(
                "UI_PROVIDER_REMOVE state=audit_failed owner={:?} runtime_refs={} callback_refs={} integrity_ok={} shell_aligned={}",
                owner,
                runtime_references,
                callback_references,
                integrity_ok,
                shell_aligned,
            );
            return;
        }
        let finalized = self
            .shell
            .finalize_provider_removal(pending, ProviderRuntimeAudit::verified(owner));
        match finalized {
            Ok(purge) => {
                self.provider_fixture_state = ProviderFixtureState::Removed;
                self.navigation_faulted = false;
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=finalized owner={:?} definitions={} overlays={} queued={}",
                    owner,
                    purge.definitions,
                    purge.composition.live_overlays,
                    purge.composition.queued_modals,
                );
                self.log_lifecycle_checkpoint("provider_finalized", 0);
            }
            Err(error) => {
                self.navigation_faulted = true;
                self.lifecycle_audit_faulted = true;
                esp_println::println!(
                    "UI_PROVIDER_REMOVE state=audit_failed owner={:?} error={:?}",
                    owner,
                    error,
                );
            }
        }
    }

    #[cfg(feature = "ui-provider-fixture")]
    fn clear_provider_fixture_refs(&mut self) {
        self.surfaces.provider_fixture = self.surfaces.home;
        self.surfaces.provider_overlay = self.surfaces.confirm;
    }

    pub(crate) fn active_surface_label(&self) -> Option<&'static str> {
        let surface = self.shell.active().surface;
        if surface == self.surfaces.home {
            Some("home")
        } else if surface == self.surfaces.launcher {
            Some("launcher")
        } else if surface == self.surfaces.diagnostics {
            Some("diagnostics")
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

    fn read_multitouch(
        &mut self,
        display: &mut InkplateDriver,
        batch: LvglContactBatch,
        cleanup: bool,
    ) -> Option<DirtyArea> {
        self.render_with(display, |_| unsafe {
            let input = LVGL_INPUT.load(Ordering::Acquire);
            if input.is_null() {
                return;
            }
            io::queue_multitouch(batch);
            lv::lv_indev_read(input);
            if cleanup {
                // Advance ENDED/CANCELED recognizers back to NONE before the
                // single-touch path resumes ownership of the pointer indev.
                io::queue_multitouch(LvglContactBatch::default());
                lv::lv_indev_read(input);
            }
        })
    }

    fn render_with(
        &mut self,
        display: &mut InkplateDriver,
        update: impl FnOnce(&mut Self),
    ) -> Option<DirtyArea> {
        io::begin(display);
        update(self);
        self.drain_navigation();
        if self.active_surface_is_renderable() {
            unsafe {
                let lv_display = LVGL_DISPLAY.load(Ordering::Acquire);
                if !lv_display.is_null() {
                    lv::lv_refr_now(lv_display);
                }
            }
        }
        io::finish()
    }

    fn drain_navigation(&mut self) {
        self.retry_overlay_cleanup();
        self.retry_blocked_cleanup();
        #[cfg(feature = "ui-provider-fixture")]
        self.try_finalize_provider_fixture_removal();
        if intent_bridge::take_overflowed() {
            esp_println::println!("UI_NAV state=rejected reason=callback_queue_full");
        }
        while let Some(action) = intent_bridge::take_intent() {
            match action {
                OwnedShellIntent::Navigate(intent) => {
                    if let Err(error) = self.shell.queue_intent(intent) {
                        esp_println::println!(
                            "UI_NAV state=rejected reason=shell_queue error={:?}",
                            error
                        );
                    }
                    self.drain_shell_navigation();
                }
                OwnedShellIntent::Compose(intent) => self.drain_composition_intent(intent),
                OwnedShellIntent::Refresh(intent) => match intent.intent {
                    RefreshIntent::FullRepaint => intent_bridge::mark_full_repaint_requested(),
                },
            }
        }
        self.drain_shell_navigation();
    }

    fn drain_composition_intent(&mut self, owned: OwnedCompositionIntent) {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            esp_println::println!(
                "UI_COMPOSITION state=rejected reason=cleanup_blocked source={:?}",
                owned.source,
            );
            return;
        }
        let prepared = match self.shell.prepare_composition_intent(owned) {
            Ok(prepared) => prepared,
            Err(error) => {
                esp_println::println!(
                    "UI_COMPOSITION state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                    owned.source,
                    owned.intent,
                    error,
                );
                return;
            }
        };
        match prepared.result() {
            CompositionPlanResult::Admission(OverlayAdmission::Queued(instance)) => {
                match self.shell.commit_overlay_request(prepared) {
                    Ok(OverlayAdmission::Queued(committed)) if committed == instance => {
                        esp_println::println!(
                            "UI_COMPOSITION state=queued token={:?}",
                            committed.token,
                        );
                    }
                    Ok(_) | Err(_) => {
                        esp_println::println!(
                            "UI_COMPOSITION state=rejected reason=queued_commit token={:?}",
                            instance.token,
                        );
                    }
                }
            }
            CompositionPlanResult::Admission(OverlayAdmission::Active(instance)) => {
                self.enter_overlay(prepared, instance);
            }
            CompositionPlanResult::Removal(dismissal) if !dismissal.removed_was_live => {
                match self.shell.commit_overlay_removal(prepared) {
                    Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {
                        esp_println::println!(
                            "UI_COMPOSITION state=removed_queued token={:?}",
                            dismissal.removed.token,
                        );
                    }
                    Ok(_) | Err(_) => {
                        esp_println::println!(
                            "UI_COMPOSITION state=rejected reason=queued_remove_commit token={:?}",
                            dismissal.removed.token,
                        );
                    }
                }
            }
            CompositionPlanResult::Removal(dismissal) => {
                self.remove_live_overlay(prepared, dismissal);
            }
            CompositionPlanResult::Cleanup(_) => {
                esp_println::println!(
                    "UI_COMPOSITION state=rejected reason=unexpected_cleanup_intent"
                );
            }
        }
    }

    fn enter_overlay(
        &mut self,
        prepared: PreparedComposition<LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY>,
        instance: OverlayInstance,
    ) {
        let mut departures = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for departing in prepared.delta().leave_live() {
            departures
                .push(departing.token)
                .expect("an admission delta is bounded by live overlay capacity");
        }
        if self.overlays.len().saturating_sub(departures.len()) == LIVE_OVERLAY_CAPACITY {
            esp_println::println!("UI_COMPOSITION state=rejected reason=runtime_capacity");
            return;
        }
        let mut candidates = match self.stage_overlay_entries(prepared.delta().enter_live()) {
            Some(candidates) => candidates,
            None => {
                esp_println::println!(
                    "UI_COMPOSITION state=rejected reason=entry token={:?}",
                    instance.token,
                );
                return;
            }
        };
        if !self.stage_overlay_departures(&departures) {
            self.destroy_uncommitted_overlays(&mut candidates);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            esp_println::println!(
                "UI_COMPOSITION state=fault reason=preemption_runtime_misaligned token={:?}",
                instance.token,
            );
            return;
        }
        match self.shell.commit_overlay_request(prepared) {
            Ok(OverlayAdmission::Active(committed)) if committed == instance => {
                self.complete_overlay_departures(&departures);
                self.activate_overlay_entries(&mut candidates);
                esp_println::println!(
                    "UI_COMPOSITION state=active token={:?} input={:?} preempted={}",
                    instance.token,
                    instance.input,
                    departures.len(),
                );
            }
            Ok(_) | Err(_) => {
                self.restore_overlay_departures(&departures);
                self.destroy_uncommitted_overlays(&mut candidates);
                esp_println::println!(
                    "UI_COMPOSITION state=rejected reason=commit token={:?}",
                    instance.token,
                );
            }
        }
    }

    fn remove_live_overlay(
        &mut self,
        prepared: PreparedComposition<LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY>,
        dismissal: OverlayDismissal,
    ) {
        let mut promotion = match dismissal.promoted {
            Some(instance) => match self.create_overlay_candidate(instance) {
                Ok(candidate) => Some(candidate),
                Err(error) => {
                    esp_println::println!(
                        "UI_COMPOSITION state=rejected reason=promotion_entry token={:?} error={:?}",
                        instance.token,
                        error,
                    );
                    return;
                }
            },
            None => None,
        };
        if promotion
            .as_ref()
            .is_some_and(|candidate| candidate.enable().is_err())
        {
            self.navigation_faulted = true;
            self.composition_faulted = true;
            self.destroy_uncommitted_overlay(
                promotion
                    .take()
                    .expect("the failed promotion remains owned"),
            );
            return;
        }
        let Some(index) = self
            .overlays
            .iter()
            .position(|overlay| overlay.token() == dismissal.removed.token)
        else {
            if let Some(candidate) = promotion {
                self.destroy_uncommitted_overlay(candidate);
            }
            self.navigation_faulted = true;
            self.composition_faulted = true;
            esp_println::println!(
                "UI_COMPOSITION state=fault reason=missing_runtime token={:?}",
                dismissal.removed.token,
            );
            return;
        };
        if self.overlays[index].disable().is_err() {
            self.navigation_faulted = true;
            self.composition_faulted = true;
            if let Some(candidate) = promotion {
                self.destroy_uncommitted_overlay(candidate);
            }
            esp_println::println!(
                "UI_COMPOSITION state=rejected reason=quiesce token={:?}",
                dismissal.removed.token,
            );
            return;
        }
        self.overlays[index].hide();
        match self.shell.commit_overlay_removal(prepared) {
            Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {}
            Ok(_) | Err(_) => {
                self.overlays[index].show();
                if self.overlays[index].enable().is_err() {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                }
                if let Some(candidate) = promotion {
                    self.destroy_uncommitted_overlay(candidate);
                }
                esp_println::println!(
                    "UI_COMPOSITION state=rolled_back token={:?}",
                    dismissal.removed.token,
                );
                return;
            }
        }

        let removed = self.overlays.remove(index);
        if let Some(candidate) = promotion {
            candidate.show();
            if self.overlays.push(candidate).is_err() {
                panic!("bounded promoted overlay ownership overflowed after live removal");
            }
        }
        let destroy_ok = self.destroy_committed_overlay(removed);
        if dismissal.promoted.is_none() && destroy_ok {
            set_system_layer_capture(false);
        }
        esp_println::println!(
            "UI_COMPOSITION state=dismissed token={:?} promoted={:?} cleanup_blocked={}",
            dismissal.removed.token,
            dismissal.promoted.map(|instance| instance.token),
            !self.overlay_cleanup_blocked.is_empty(),
        );
    }

    fn create_overlay_candidate(
        &self,
        instance: OverlayInstance,
    ) -> Result<ActiveOverlay, OverlayEnterError> {
        if instance.token.surface == self.surfaces.confirm && instance.input == OverlayInput::Modal
        {
            ActiveOverlay::confirm(instance)
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
                ActiveOverlay::confirm(instance)
            }
            #[cfg(not(feature = "ui-provider-fixture"))]
            {
                Err(OverlayEnterError::ObjectCreation)
            }
        } else {
            Err(OverlayEnterError::ObjectCreation)
        }
    }

    fn destroy_uncommitted_overlay(&mut self, overlay: ActiveOverlay) {
        match overlay.destroy() {
            Ok(()) => {}
            Err(DestroyFailure::Live(overlay)) => {
                if self.overlay_cleanup_blocked.push(overlay).is_err() {
                    panic!("bounded overlay cleanup ownership overflowed");
                }
                self.navigation_faulted = true;
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.composition_faulted = true;
            }
        }
    }

    fn destroy_committed_overlay(&mut self, overlay: ActiveOverlay) -> bool {
        match overlay.destroy() {
            Ok(()) => true,
            Err(DestroyFailure::Live(overlay)) => {
                if self.overlay_cleanup_blocked.push(overlay).is_err() {
                    panic!("bounded overlay cleanup ownership overflowed");
                }
                self.navigation_faulted = true;
                false
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                false
            }
        }
    }

    fn drain_shell_navigation(&mut self) {
        if self.navigation_faulted
            || self.composition_faulted
            || self.cleanup_blocked.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
        {
            while self.shell.pop_intent().is_some() {}
            esp_println::println!("UI_NAV state=rejected reason=cleanup_blocked");
            return;
        }

        while let Some(owned) = self.shell.pop_intent() {
            #[cfg(feature = "ui-provider-fixture")]
            if matches!(owned.intent, NavIntent::Home)
                && matches!(
                    self.provider_fixture_state,
                    ProviderFixtureState::Registered(owner)
                        if owned.source.surface.owner == owner
                )
            {
                self.execute_provider_removal_fixture();
                if self.navigation_faulted {
                    break;
                }
                continue;
            }
            if self.shell.active_modal().is_some() {
                esp_println::println!(
                    "UI_NAV state=rejected reason=modal_active source={:?} intent={:?}",
                    owned.source,
                    owned.intent,
                );
                continue;
            }
            let transition_started_us = Instant::now().as_micros();
            let prepared = match self.shell.prepare_intent(owned.intent) {
                Ok(prepared) => prepared,
                Err(error) => {
                    esp_println::println!(
                        "UI_NAV state=rejected reason=prepare source={:?} intent={:?} error={:?}",
                        owned.source,
                        owned.intent,
                        error
                    );
                    continue;
                }
            };
            let mut entering_overlays =
                match self.stage_overlay_entries(prepared.composition_delta().enter_live()) {
                    Some(candidates) => candidates,
                    None => {
                        esp_println::println!(
                            "UI_NAV state=rejected reason=overlay_promotion_entry intent={:?}",
                            owned.intent
                        );
                        continue;
                    }
                };
            let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
            for instance in prepared.composition_delta().leave_live() {
                departing_overlays
                    .push(instance.token)
                    .expect("the navigation delta is bounded by live overlay capacity");
            }
            if !self.stage_overlay_departures(&departing_overlays) {
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                esp_println::println!(
                    "UI_NAV state=fault reason=overlay_runtime_misaligned intent={:?}",
                    owned.intent
                );
                break;
            }
            let origin = prepared.origin();
            let destination = prepared.destination();
            let active_instance = self.active.as_ref().map(|active| active.token);
            if active_instance.is_some_and(|active| !prepared.requires_reentry(active)) {
                match self.shell.commit_navigation(prepared) {
                    Ok(outcome) => {
                        self.complete_overlay_departures(&departing_overlays);
                        self.activate_overlay_entries(&mut entering_overlays);
                        esp_println::println!(
                            "UI_NAV state=committed from={:?} to={:?} role={:?} outcome={:?}",
                            origin.surface,
                            destination.surface,
                            destination.role,
                            outcome
                        );
                    }
                    Err(error) => {
                        self.restore_overlay_departures(&departing_overlays);
                        self.destroy_uncommitted_overlays(&mut entering_overlays);
                        esp_println::println!(
                            "UI_NAV state=rolled_back from={:?} attempted={:?} error={:?}",
                            origin.surface,
                            destination.surface,
                            error
                        );
                    }
                }
                continue;
            }

            let destination_instance = prepared.destination_instance();
            let Some(origin_instance) = self.active.take() else {
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.navigation_faulted = true;
                esp_println::println!("UI_NAV state=fault reason=missing_active_instance");
                break;
            };
            let mut runtime = LvglSurfaceRuntime {
                surfaces: self.surfaces,
                transition_started_us,
            };
            let result = execute_transition(
                &mut runtime,
                origin_instance,
                destination,
                destination_instance,
                || self.shell.commit_navigation(prepared),
            );
            let transition_us = Instant::now()
                .as_micros()
                .saturating_sub(transition_started_us);

            match result {
                TransitionResult::Committed { active, outcome } => {
                    self.active = Some(active);
                    self.complete_overlay_departures(&departing_overlays);
                    self.activate_overlay_entries(&mut entering_overlays);
                    esp_println::println!(
                        "UI_NAV state=committed from={:?} to={:?} role={:?} outcome={:?} transition_us={} cleanup_blocked={}",
                        origin.surface,
                        destination.surface,
                        destination.role,
                        outcome,
                        transition_us,
                        !self.overlay_cleanup_blocked.is_empty(),
                    );
                    self.log_lifecycle_checkpoint("settled_after_delete", transition_us);
                }
                TransitionResult::RolledBack {
                    active,
                    cleanup_blocked,
                    cleanup_audit_failed,
                    reason,
                } => {
                    self.lifecycle_audit_faulted |= cleanup_audit_failed;
                    self.restore_overlay_departures(&departing_overlays);
                    self.destroy_uncommitted_overlays(&mut entering_overlays);
                    let origin_restored = match &reason {
                        RollbackReason::Entry(_) => true,
                        RollbackReason::Activation { origin_restored }
                        | RollbackReason::Quiesce { origin_restored }
                        | RollbackReason::CandidateEnable { origin_restored }
                        | RollbackReason::Commit {
                            origin_restored, ..
                        } => *origin_restored,
                    };
                    self.cleanup_blocked = cleanup_blocked;
                    if origin_restored {
                        self.active = Some(active);
                        self.navigation_faulted =
                            self.cleanup_blocked.is_some() || cleanup_audit_failed;
                    } else if self.cleanup_blocked.is_none() && !cleanup_audit_failed {
                        self.navigation_faulted = !self.recover_ambient(active);
                    } else {
                        self.active = Some(active);
                        self.navigation_faulted = true;
                    }
                    esp_println::println!(
                        "UI_NAV state=rolled_back from={:?} attempted={:?} reason={:?} transition_us={} cleanup_blocked={} navigation_faulted={}",
                        origin.surface,
                        destination.surface,
                        reason,
                        transition_us,
                        self.cleanup_blocked.is_some(),
                        self.navigation_faulted,
                    );
                    self.log_lifecycle_checkpoint("rolled_back", transition_us);
                }
                TransitionResult::FaultedAfterCommit {
                    active,
                    cleanup_blocked,
                    cleanup_audit_failed,
                    outcome,
                    reason,
                } => {
                    self.lifecycle_audit_faulted |= cleanup_audit_failed;
                    self.active = Some(active);
                    self.complete_overlay_departures(&departing_overlays);
                    self.activate_overlay_entries(&mut entering_overlays);
                    self.cleanup_blocked = cleanup_blocked;
                    self.navigation_faulted = true;
                    esp_println::println!(
                        "UI_NAV state=faulted_after_commit from={:?} to={:?} outcome={:?} reason={:?} transition_us={} cleanup_audit_failed={}",
                        origin.surface,
                        destination.surface,
                        outcome,
                        reason,
                        transition_us,
                        cleanup_audit_failed,
                    );
                    self.log_lifecycle_checkpoint("cleanup_blocked", transition_us);
                }
            }
            if self.navigation_faulted {
                break;
            }
        }
    }

    fn recover_ambient(&mut self, origin: ActiveSurface) -> bool {
        let transition_started_us = Instant::now().as_micros();
        let prepared = match self.shell.prepare_recovery_home() {
            Ok(prepared) => prepared,
            Err(error) => {
                self.active = Some(origin);
                esp_println::println!(
                    "UI_NAV state=recovery_failed stage=prepare error={:?}",
                    error
                );
                return false;
            }
        };
        let mut entering_overlays = match self
            .stage_overlay_entries(prepared.composition_delta().enter_live())
        {
            Some(candidates) => candidates,
            None => {
                self.active = Some(origin);
                esp_println::println!("UI_NAV state=recovery_failed stage=overlay_promotion_entry");
                return false;
            }
        };
        let mut departing_overlays = Vec::<SurfaceInstanceToken, LIVE_OVERLAY_CAPACITY>::new();
        for instance in prepared.composition_delta().leave_live() {
            departing_overlays
                .push(instance.token)
                .expect("the recovery delta is bounded by live overlay capacity");
        }
        if !self.stage_overlay_departures(&departing_overlays) {
            self.destroy_uncommitted_overlays(&mut entering_overlays);
            self.active = Some(origin);
            esp_println::println!("UI_NAV state=recovery_failed stage=overlay_runtime_misaligned");
            return false;
        }
        let destination = prepared.destination();
        let destination_instance = prepared.destination_instance();
        let mut runtime = LvglSurfaceRuntime {
            surfaces: self.surfaces,
            transition_started_us,
        };
        match execute_transition(
            &mut runtime,
            origin,
            destination,
            destination_instance,
            || self.shell.commit_navigation(prepared),
        ) {
            TransitionResult::Committed { active, outcome } => {
                self.active = Some(active);
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.cleanup_blocked = None;
                esp_println::println!(
                    "UI_NAV state=recovered destination={:?} outcome={:?}",
                    destination.surface,
                    outcome
                );
                self.active_surface_is_renderable()
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.restore_overlay_departures(&departing_overlays);
                self.destroy_uncommitted_overlays(&mut entering_overlays);
                self.active = Some(active);
                self.cleanup_blocked = cleanup_blocked;
                esp_println::println!(
                    "UI_NAV state=recovery_failed stage=transition reason={:?} cleanup_blocked={} cleanup_audit_failed={}",
                    reason,
                    self.cleanup_blocked.is_some(),
                    cleanup_audit_failed
                );
                false
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                outcome,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active = Some(active);
                self.complete_overlay_departures(&departing_overlays);
                self.activate_overlay_entries(&mut entering_overlays);
                self.cleanup_blocked = cleanup_blocked;
                esp_println::println!(
                    "UI_NAV state=recovery_cleanup_blocked outcome={:?} reason={:?} cleanup_audit_failed={}",
                    outcome,
                    reason,
                    cleanup_audit_failed
                );
                false
            }
        }
    }

    fn stage_overlay_entries(
        &mut self,
        instances: &[OverlayInstance],
    ) -> Option<Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>> {
        let mut candidates = Vec::new();
        for instance in instances {
            let candidate = match self.create_overlay_candidate(*instance) {
                Ok(candidate) => candidate,
                Err(_) => {
                    self.destroy_uncommitted_overlays(&mut candidates);
                    return None;
                }
            };
            if candidate.enable().is_err() {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                self.destroy_uncommitted_overlay(candidate);
                self.destroy_uncommitted_overlays(&mut candidates);
                return None;
            }
            if let Err(candidate) = candidates.push(candidate) {
                self.destroy_uncommitted_overlay(candidate);
                self.destroy_uncommitted_overlays(&mut candidates);
                return None;
            }
        }
        Some(candidates)
    }

    fn activate_overlay_entries(
        &mut self,
        candidates: &mut Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    ) {
        while !candidates.is_empty() {
            let candidate = candidates.remove(0);
            if candidate.is_modal() {
                set_system_layer_capture(true);
            }
            candidate.show();
            if let Err(candidate) = self.overlays.push(candidate) {
                self.destroy_uncommitted_overlay(candidate);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                panic!("committed overlay entries exceeded the bounded runtime capacity");
            }
        }
    }

    fn destroy_uncommitted_overlays(
        &mut self,
        candidates: &mut Vec<ActiveOverlay, LIVE_OVERLAY_CAPACITY>,
    ) {
        while let Some(candidate) = candidates.pop() {
            self.destroy_uncommitted_overlay(candidate);
        }
    }

    fn stage_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) -> bool {
        for (staged, token) in tokens.iter().enumerate() {
            let Some(index) = self
                .overlays
                .iter()
                .position(|overlay| overlay.token() == *token)
            else {
                self.restore_overlay_departures(&tokens[..staged]);
                return false;
            };
            if self.overlays[index].disable().is_err() {
                self.restore_overlay_departures(&tokens[..staged]);
                self.navigation_faulted = true;
                self.composition_faulted = true;
                return false;
            }
            self.overlays[index].hide();
        }
        true
    }

    fn restore_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) {
        for token in tokens {
            if let Some(overlay) = self
                .overlays
                .iter()
                .find(|overlay| overlay.token() == *token)
            {
                overlay.show();
                if overlay.enable().is_err() {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                }
            }
        }
    }

    fn complete_overlay_departures(&mut self, tokens: &[SurfaceInstanceToken]) {
        for token in tokens {
            let Some(index) = self
                .overlays
                .iter()
                .position(|overlay| overlay.token() == *token)
            else {
                self.navigation_faulted = true;
                self.composition_faulted = true;
                esp_println::println!(
                    "UI_COMPOSITION state=fault reason=missing_runtime token={:?}",
                    token
                );
                continue;
            };
            let overlay = self.overlays.remove(index);
            match overlay.destroy() {
                Ok(()) => {}
                Err(DestroyFailure::Live(overlay)) => {
                    if self.overlay_cleanup_blocked.push(overlay).is_err() {
                        panic!("bounded overlay cleanup ownership overflowed");
                    }
                    self.navigation_faulted = true;
                }
                Err(DestroyFailure::Audit) => {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                    esp_println::println!(
                        "UI_COMPOSITION state=fault reason=unexpected_overlay_audit"
                    );
                }
            }
        }
        if !self.navigation_faulted
            && self.shell.active_modal().is_none()
            && self.overlays.iter().all(|overlay| !overlay.is_modal())
            && self
                .overlay_cleanup_blocked
                .iter()
                .all(|overlay| !overlay.is_modal())
        {
            set_system_layer_capture(false);
        }
    }

    fn retry_overlay_cleanup(&mut self) {
        if self.overlay_cleanup_blocked.is_empty() {
            return;
        }
        let mut blocked = core::mem::replace(&mut self.overlay_cleanup_blocked, Vec::new());
        let mut audit_failed = false;
        while let Some(overlay) = blocked.pop() {
            match overlay.destroy() {
                Ok(()) => {}
                Err(DestroyFailure::Live(overlay)) => {
                    if self.overlay_cleanup_blocked.push(overlay).is_err() {
                        panic!("bounded overlay cleanup ownership overflowed");
                    }
                }
                Err(DestroyFailure::Audit) => {
                    self.navigation_faulted = true;
                    self.composition_faulted = true;
                    audit_failed = true;
                }
            }
        }
        if self.overlay_cleanup_blocked.is_empty()
            && self.cleanup_blocked.is_none()
            && self.active_surface_is_renderable()
            && !audit_failed
            && !self.composition_faulted
            && !self.lifecycle_audit_faulted
        {
            self.navigation_faulted = false;
        }
        if !self.navigation_faulted
            && self.shell.active_modal().is_none()
            && self.overlays.iter().all(|overlay| !overlay.is_modal())
        {
            set_system_layer_capture(false);
        }
    }

    fn retry_blocked_cleanup(&mut self) {
        let Some(blocked) = self.cleanup_blocked.take() else {
            return;
        };
        match blocked.destroy() {
            Ok(()) => {
                if self.composition_faulted || self.lifecycle_audit_faulted {
                    self.navigation_faulted = true;
                } else if self.overlay_cleanup_blocked.is_empty()
                    && self.active_surface_is_renderable()
                {
                    self.navigation_faulted = false;
                } else if let Some(origin) = self.active.take() {
                    self.navigation_faulted = !self.recover_ambient(origin);
                } else {
                    self.navigation_faulted = true;
                }
                esp_println::println!(
                    "UI_NAV state=cleanup_recovered navigation_faulted={}",
                    self.navigation_faulted
                );
            }
            Err(DestroyFailure::Live(blocked)) => {
                self.cleanup_blocked = Some(blocked);
                self.navigation_faulted = true;
            }
            Err(DestroyFailure::Audit) => {
                self.navigation_faulted = true;
                self.lifecycle_audit_faulted = true;
                esp_println::println!("UI_NAV state=fault reason=cleanup_route_audit");
            }
        }
    }

    fn active_surface_is_renderable(&self) -> bool {
        self.active.as_ref().is_some_and(|instance| {
            let root = instance.root();
            !root.is_null()
                && unsafe { lv::lv_obj_is_valid(root) && lv::lv_screen_active() == root }
                && instance.frame == self.shell.active()
                && instance.token == self.shell.active_instance()
        })
    }

    fn log_lifecycle_checkpoint(&self, phase: &str, transition_us: u64) {
        telemetry::record_stack_headroom();
        let mut monitor = MaybeUninit::<lv::lv_mem_monitor_t>::zeroed();
        let integrity_ok = unsafe { lv::lv_mem_test() == lv::lv_result_t_LV_RESULT_OK };
        unsafe { lv::lv_mem_monitor(monitor.as_mut_ptr()) };
        let monitor = unsafe { monitor.assume_init() };
        let allocator = psram::allocator_memory_snapshot();
        let active = self.active.as_ref().map(|instance| instance.token);
        let shell_aligned = self.active.as_ref().is_some_and(|instance| {
            instance.frame == self.shell.active() && instance.token == self.shell.active_instance()
        });
        esp_println::println!(
            "LVGL_LIFECYCLE phase={} active={:?} shell_aligned={} transition_us={} lvgl_total={} lvgl_used={} lvgl_free={} lvgl_biggest_free={} lvgl_used_blocks={} lvgl_free_blocks={} lvgl_max_used={} lvgl_frag_pct={} integrity_ok={} heap_internal_free={} heap_internal_min={} heap_external_free={} heap_external_min={} heap_peak_used={} cpu0_stack_min={} timer_gap_max_us={} timer_runtime_max_us={} cleanup_blocked={} navigation_faulted={} composition_faulted={} lifecycle_audit_faulted={}",
            phase,
            active,
            shell_aligned,
            transition_us,
            monitor.total_size,
            monitor.total_size.saturating_sub(monitor.free_size),
            monitor.free_size,
            monitor.free_biggest_size,
            monitor.used_cnt,
            monitor.free_cnt,
            monitor.max_used,
            monitor.frag_pct,
            integrity_ok,
            allocator.free_internal_bytes,
            allocator.min_free_internal_bytes,
            allocator.free_external_bytes,
            allocator.min_free_external_bytes,
            allocator.peak_used_bytes,
            telemetry::minimum_stack_headroom_bytes(),
            self.timer_metrics.max_gap_us(),
            self.timer_metrics.max_runtime_us(),
            self.cleanup_blocked.is_some() || !self.overlay_cleanup_blocked.is_empty(),
            self.navigation_faulted,
            self.composition_faulted,
            self.lifecycle_audit_faulted,
        );
    }
}

fn activate_root(root: *mut lv::lv_obj_t) -> bool {
    if root.is_null() {
        return false;
    }
    unsafe {
        lv::lv_screen_load(root);
        lv::lv_screen_active() == root
    }
}

fn set_system_layer_capture(enabled: bool) {
    let layer = unsafe { lv::lv_layer_sys() };
    if layer.is_null() {
        return;
    }
    unsafe {
        if enabled {
            lv::lv_obj_add_flag(layer, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        } else {
            lv::lv_obj_remove_flag(layer, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        }
    }
}

fn destroy_initial_surface_or_stop(surface: ActiveSurface) {
    match surface.destroy() {
        Ok(()) => {}
        Err(DestroyFailure::Live(_)) => {
            panic!("initial LVGL surface remained live after cleanup")
        }
        Err(DestroyFailure::Audit) => {
            panic!("initial LVGL callback route audit failed after cleanup")
        }
    }
}

fn destroy_initial_overlay_or_stop(overlay: ActiveOverlay) {
    match overlay.destroy() {
        Ok(()) => {}
        Err(DestroyFailure::Live(_)) => {
            panic!("initial LVGL overlay remained live after cleanup")
        }
        Err(DestroyFailure::Audit) => {
            panic!("initial LVGL overlay reported an impossible audit failure")
        }
    }
}

fn destroy_initial_backend_or_stop(mut backend: Backend) {
    while let Some(overlay) = backend.overlay_cleanup_blocked.pop() {
        destroy_initial_overlay_or_stop(overlay);
    }
    while let Some(overlay) = backend.overlays.pop() {
        destroy_initial_overlay_or_stop(overlay);
    }
    if let Some(surface) = backend.cleanup_blocked.take() {
        destroy_initial_surface_or_stop(surface);
    }
    if let Some(surface) = backend.active.take() {
        destroy_initial_surface_or_stop(surface);
    }
}

fn log_lifecycle_resources(phase: &str, transition_us: u64) {
    telemetry::record_stack_headroom();
    let mut monitor = MaybeUninit::<lv::lv_mem_monitor_t>::zeroed();
    let integrity_ok = unsafe { lv::lv_mem_test() == lv::lv_result_t_LV_RESULT_OK };
    unsafe { lv::lv_mem_monitor(monitor.as_mut_ptr()) };
    let monitor = unsafe { monitor.assume_init() };
    let allocator = psram::allocator_memory_snapshot();
    esp_println::println!(
        "LVGL_LIFECYCLE phase={} transition_us={} lvgl_total={} lvgl_used={} lvgl_free={} lvgl_biggest_free={} lvgl_used_blocks={} lvgl_free_blocks={} lvgl_max_used={} lvgl_frag_pct={} integrity_ok={} heap_internal_free={} heap_internal_min={} heap_external_free={} heap_external_min={} heap_peak_used={} cpu0_stack_min={}",
        phase,
        transition_us,
        monitor.total_size,
        monitor.total_size.saturating_sub(monitor.free_size),
        monitor.free_size,
        monitor.free_biggest_size,
        monitor.used_cnt,
        monitor.free_cnt,
        monitor.max_used,
        monitor.frag_pct,
        integrity_ok,
        allocator.free_internal_bytes,
        allocator.min_free_internal_bytes,
        allocator.free_external_bytes,
        allocator.min_free_external_bytes,
        allocator.peak_used_bytes,
        telemetry::minimum_stack_headroom_bytes(),
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
