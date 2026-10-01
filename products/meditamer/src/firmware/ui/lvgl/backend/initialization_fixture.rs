//! Boot-only checks through the actual Backend and LVGL, on the display task.
//! Injected boundary errors exercise cleanup; they do not simulate physical OOM.

use core::sync::atomic::AtomicU8;

use super::*;

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(super) enum Stage {
    Session = 1,
    Display,
    Input,
    NavigationOverlay,
    RefreshOverlay,
}

static REJECT_STAGE: AtomicU8 = AtomicU8::new(0);

pub(super) fn checkpoint(stage: Stage) -> Result<(), InitError> {
    if REJECT_STAGE
        .compare_exchange(stage as u8, 0, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return Ok(());
    }
    Err(stage.error())
}

impl Stage {
    fn error(self) -> InitError {
        match self {
            Self::Session => InitError::DisplayCreationFailed,
            Self::Display => InitError::InputCreationFailed,
            Self::Input => InitError::SurfaceCreationFailed,
            Self::NavigationOverlay | Self::RefreshOverlay => InitError::ShellConfigurationFailed,
        }
    }
}

fn external_free() -> usize {
    esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::External.into())
}

fn assert_clean(destination: &Option<Backend>, baseline: usize) {
    assert!(destination.is_none());
    assert!(!render::lvgl_adapter::is_initialized());
    assert_eq!(external_free(), baseline);
    assert!(intent_bridge::take_intent().is_none());
}

/// Invoke the real bridge callback through a checked LVGL widget. The opaque
/// route identity is not an address of the Backend or its external owner.
fn click_route(backend: &Backend, route: intent_bridge::CallbackIdentity) {
    let button = backend
        .coordinator
        .active_screen()
        .unwrap()
        .root_widget()
        .child(&backend.ui_token, render::lvgl_adapter::WidgetKind::Button)
        .expect("fixture button");
    intent_bridge::bind_navigation_identity(
        &button,
        &backend.ui_token,
        route,
        intent_bridge::HOME_NAVIGATION_INDEX,
    )
    .expect("fixture callback binding");
    assert!(button.send_click(&backend.ui_token).unwrap_or(false));
    button
        .delete(&backend.ui_token)
        .expect("fixture button cleanup");
}

fn teardown(destination: &mut Option<Backend>) {
    destroy_initial_backend_or_stop(destination.take().unwrap());
}

impl Backend {
    #[inline(never)]
    pub(crate) fn verify_initialization(
        destination: &mut Option<Self>,
        display: &mut InkplateDriver,
    ) {
        assert!(destination.is_none());
        // This allocation intentionally outlives every LVGL session.
        prepare_memory_pool().unwrap();
        let baseline = external_free();
        for round in 0..2 {
            for stage in [
                Stage::Session,
                Stage::Display,
                Stage::Input,
                Stage::NavigationOverlay,
                Stage::RefreshOverlay,
            ] {
                REJECT_STAGE.store(stage as u8, Ordering::Relaxed);
                assert_eq!(
                    Self::initialize_into(destination, display, PersistedUiSettings::default()),
                    Err(stage.error()),
                );
                assert_eq!(REJECT_STAGE.load(Ordering::Relaxed), 0);
                assert_clean(destination, baseline);
                console::println!(
                    "UI_INIT_FIXTURE round={} stage={:?} outcome=passed",
                    round,
                    stage
                );
            }
        }

        Self::initialize_into(destination, display, PersistedUiSettings::default()).unwrap();
        let active = destination
            .as_ref()
            .unwrap()
            .coordinator
            .active_screen()
            .unwrap();
        let root = active.root_widget().identity();
        let route = active.callbacks.identity();
        let source = active.token;
        assert_eq!(
            Self::initialize_into(destination, display, PersistedUiSettings::default()),
            Err(InitError::AlreadyInitialized),
        );
        let backend = destination.as_ref().unwrap();
        assert_eq!(
            backend
                .coordinator
                .active_screen()
                .unwrap()
                .root_widget()
                .identity(),
            root
        );
        click_route(backend, route);
        assert_eq!(
            intent_bridge::take_intent(),
            Some(OwnedShellIntent::Navigate(OwnedNavIntent {
                source,
                intent: NavIntent::Home
            })),
        );
        // Teardown must also purge an already queued callback intent.
        click_route(backend, route);
        teardown(destination);
        assert_clean(destination, baseline);
        console::println!("UI_INIT_FIXTURE stage=installed_callback_and_teardown outcome=passed");

        Self::initialize_into(destination, display, PersistedUiSettings::default()).unwrap();
        let backend = destination.as_ref().unwrap();
        click_route(backend, route);
        assert!(intent_bridge::take_intent().is_none());
        let active = backend.coordinator.active_screen().unwrap();
        click_route(backend, active.callbacks.identity());
        assert_eq!(
            intent_bridge::take_intent(),
            Some(OwnedShellIntent::Navigate(OwnedNavIntent {
                source: active.token,
                intent: NavIntent::Home,
            }))
        );
        assert!(backend
            .ui_token
            .memory_snapshot()
            .is_ok_and(|snapshot| snapshot.integrity_ok));
        teardown(destination);
        assert_clean(destination, baseline);
        console::println!(
            "UI_INIT_FIXTURE stage=stale_callback_and_reinitialization outcome=passed"
        );
    }
}
