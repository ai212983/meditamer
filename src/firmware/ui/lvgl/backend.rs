//! LVGL backend.
//!
//! This file owns the backend's data model -- the surface set, the active
//! surface and overlay state, and the [`Backend`] handle itself. The work is
//! split by responsibility: [`init`] brings LVGL up, [`frame`] drives one
//! frame, [`navigation`] and [`overlay`] advance the shell's surface graph,
//! and [`cycle`] serves the serial-driven UI cycle and its fixture.

mod cycle;
mod frame;
mod init;
mod navigation;
mod overlay;

use core::sync::atomic::{AtomicPtr, Ordering};
use core::{mem::MaybeUninit, ptr};

use embassy_time::Instant;
use heapless::Vec;
use lightvgl_sys as lv;

use super::base_overlays::{ActiveOverlay, BaseOverlayKind, OverlayEnterError};
use super::dither::DirtyArea;
#[cfg(feature = "ui-provider-fixture")]
use super::provider_fixture;
use super::{
    ambient_picker, gesture_test, home, intent_bridge, io, launcher, overlay_settings, HEIGHT,
    WIDTH,
};
#[cfg(feature = "ui-provider-fixture")]
use crate::firmware::ui::shell::model::{PendingProviderRemoval, ProviderRuntimeAudit};
use crate::firmware::{
    psram::{self, BufferPlacement},
    telemetry,
    touch::lvgl_multitouch::{LvglContactBatch, LvglMultitouchFrame, LvglMultitouchTracker},
    touch::types::TouchEvent,
    types::InkplateDriver,
    ui::shell::{
        catalogue::{CatalogueAction, CatalogueViewKind, DefaultCatalogue, EntryId},
        composition::CompositionPlanResult,
        lifecycle::{
            execute_transition, DestroyFailure, LifecycleEvent, RollbackReason, SurfaceRuntime,
            TransitionResult,
        },
        model::{
            DefaultShellModel, PreparedComposition, LIVE_OVERLAY_CAPACITY, MODAL_QUEUE_CAPACITY,
        },
        navigator::NavigationFrame,
        settings::{PersistedUiSettings, UiSettings, UiSettingsPersistence},
        timing::TimerServiceMetrics,
        types::{
            CompositionIntent, NavIntent, OverlayAdmission, OverlayDismissal, OverlayInput,
            OverlayInstance, OverlayLifetime, OwnedCompositionIntent, OwnedNavIntent,
            OwnedShellIntent, OwnedUiSettingsIntent, ProviderId, RefreshHint, RefreshIntent,
            SurfaceCapabilities, SurfaceId, SurfaceInstanceToken, SurfaceRef, SurfaceRole,
            SurfaceSpec,
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
const AMBIENT_PICKER_SURFACE_ID: SurfaceId = SurfaceId(7);
const OVERLAY_SETTINGS_SURFACE_ID: SurfaceId = SurfaceId(8);
const BASE_NAMESPACE: u16 = 1;
const HOME_ENTRY_ID: EntryId = EntryId::new(BASE_NAMESPACE, 1);
const REFRESH_CONTROL_ENTRY_ID: EntryId = EntryId::new(BASE_NAMESPACE, 5);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ID: ProviderId = ProviderId(2);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_ROOT_ID: SurfaceId = SurfaceId(101);
#[cfg(feature = "ui-provider-fixture")]
const PROVIDER_FIXTURE_OVERLAY_ID: SurfaceId = SurfaceId(102);
const BASE_SURFACES: [SurfaceSpec; 8] = [
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
    SurfaceSpec::new(
        AMBIENT_PICKER_SURFACE_ID.0,
        SurfaceRole::SystemRoot,
        SurfaceCapabilities::LAUNCHABLE,
        RefreshHint::Boundary,
    ),
    SurfaceSpec::new(
        OVERLAY_SETTINGS_SURFACE_ID.0,
        SurfaceRole::SystemRoot,
        SurfaceCapabilities::LAUNCHABLE,
        RefreshHint::Boundary,
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
    ambient_picker: SurfaceRef,
    overlay_settings: SurfaceRef,
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
    AmbientPicker(ambient_picker::AmbientPickerScreen),
    OverlaySettings(overlay_settings::OverlaySettingsScreen),
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
        catalogue: &DefaultCatalogue,
        settings: &UiSettings,
    ) -> Result<Self, InitError> {
        #[cfg(feature = "ui-provider-fixture")]
        let requested_overlay = if frame.surface == surfaces.provider_fixture {
            surfaces.provider_overlay
        } else {
            surfaces.confirm
        };
        #[cfg(not(feature = "ui-provider-fixture"))]
        let requested_overlay = surfaces.confirm;
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
        } else if frame.surface == surfaces.ambient_picker {
            for (index, entry) in catalogue
                .view(CatalogueViewKind::AmbientPicker)
                .entries()
                .iter()
                .enumerate()
            {
                if matches!(entry.action(), CatalogueAction::Enter(_)) {
                    actions[index] = Some(intent_bridge::ScreenAction::Configure(
                        crate::firmware::ui::shell::settings::UiSettingsIntent::SelectAmbient(
                            entry.id,
                        ),
                    ));
                }
            }
        } else if frame.surface == surfaces.overlay_settings {
            for (index, entry) in catalogue
                .view(CatalogueViewKind::OverlaySettings)
                .entries()
                .iter()
                .enumerate()
            {
                if matches!(entry.action(), CatalogueAction::Enter(_)) {
                    actions[index] = Some(intent_bridge::ScreenAction::Configure(
                        crate::firmware::ui::shell::settings::UiSettingsIntent::ToggleOverlay(
                            entry.id,
                        ),
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
                lifetime: if requested_overlay == surfaces.confirm {
                    OverlayLifetime::Transient
                } else {
                    OverlayLifetime::Sticky
                },
                rank: 3,
            },
        };
        let callbacks =
            intent_bridge::claim(bindings).map_err(|_| InitError::CallbackRouteUnavailable)?;
        let user_data = callbacks.user_data();
        let model = unsafe {
            if frame.surface == surfaces.home {
                home::create(user_data).map(SurfaceModel::Home)
            } else if frame.surface == surfaces.launcher {
                launcher::create(catalogue, settings, user_data).map(SurfaceModel::Launcher)
            } else if frame.surface == surfaces.diagnostics {
                gesture_test::create(user_data).map(SurfaceModel::Diagnostics)
            } else if frame.surface == surfaces.ambient_picker {
                ambient_picker::create(catalogue, settings, user_data)
                    .map(SurfaceModel::AmbientPicker)
            } else if frame.surface == surfaces.overlay_settings {
                overlay_settings::create(catalogue, settings, user_data)
                    .map(SurfaceModel::OverlaySettings)
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

    pub(super) fn root(&self) -> *mut lv::lv_obj_t {
        match &self.model {
            SurfaceModel::Home(screen) => screen.root(),
            SurfaceModel::Launcher(screen) => screen.root(),
            SurfaceModel::Diagnostics(screen) => screen.root(),
            SurfaceModel::AmbientPicker(screen) => screen.root(),
            SurfaceModel::OverlaySettings(screen) => screen.root(),
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(screen) => screen.root(),
        }
    }

    pub(super) fn activate(&self) -> bool {
        activate_root(self.root())
    }

    pub(super) fn enable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::enable(&self.callbacks)
    }

    pub(super) fn disable(&self) -> Result<(), intent_bridge::CallbackRouteError> {
        intent_bridge::disable(&self.callbacks)
    }

    pub(super) fn destroy(self) -> Result<(), DestroyFailure<Self>> {
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

    pub(super) fn show_gesture(&mut self, event: io::LvglGestureEvent) -> bool {
        match &mut self.model {
            SurfaceModel::Diagnostics(screen) => unsafe { screen.show_gesture(event, true) },
            SurfaceModel::Home(_)
            | SurfaceModel::Launcher(_)
            | SurfaceModel::AmbientPicker(_)
            | SurfaceModel::OverlaySettings(_) => false,
            #[cfg(feature = "ui-provider-fixture")]
            SurfaceModel::ProviderFixture(_) => false,
        }
    }
}

struct LvglSurfaceRuntime<'a> {
    surfaces: SurfaceRefs,
    catalogue: &'a DefaultCatalogue,
    settings: &'a UiSettings,
    transition_started_us: u64,
}

impl SurfaceRuntime for LvglSurfaceRuntime<'_> {
    type Instance = ActiveSurface;
    type EnterError = InitError;

    fn enter(
        &mut self,
        frame: NavigationFrame,
        token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError> {
        ActiveSurface::enter(frame, token, self.surfaces, self.catalogue, self.settings)
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
    catalogue: DefaultCatalogue,
    settings: UiSettingsPersistence,
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
    pub(crate) fn take_due_settings_write(&mut self, now_ms: u64) -> Option<PersistedUiSettings> {
        self.settings.take_due(now_ms)
    }

    pub(crate) fn complete_settings_write(&mut self, success: bool, now_ms: u64) {
        self.settings.complete(success, now_ms);
    }

    pub(crate) fn active_surface_label(&self) -> Option<&'static str> {
        let surface = self.shell.active().surface;
        if surface == self.surfaces.home {
            Some("home")
        } else if surface == self.surfaces.launcher {
            Some("launcher")
        } else if surface == self.surfaces.diagnostics {
            Some("diagnostics")
        } else if surface == self.surfaces.ambient_picker {
            Some("ambient_picker")
        } else if surface == self.surfaces.overlay_settings {
            Some("overlay_settings")
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
        self.active.as_ref().is_some_and(|instance| {
            let root = instance.root();
            !root.is_null()
                && unsafe { lv::lv_obj_is_valid(root) && lv::lv_screen_active() == root }
                && instance.frame == self.shell.active()
                && instance.token == self.shell.active_instance()
        })
    }

    pub(super) fn log_lifecycle_checkpoint(&self, phase: &str, transition_us: u64) {
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
