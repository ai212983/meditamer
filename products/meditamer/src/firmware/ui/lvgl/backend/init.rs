//! Backend bring-up: LVGL init, the draw buffer, and the base surface set.

use super::*;

// Keep construction inside the allocation boundary so the compiler writes the
// zeroed pixels into their destination rather than materializing a stack copy.
#[inline(never)]
fn allocate_draw_buffer(
) -> Result<crate::firmware::psram::ExternalValue<StaticL8DrawBuffer<BUFFER_WORDS>>, InitError> {
    crate::firmware::psram::ExternalValue::try_new_with(StaticL8DrawBuffer::<BUFFER_WORDS>::new)
        .map_err(|_| InitError::MemoryPoolUnavailable)
}

impl Backend {
    /// Install only a fully initialized backend. Failures leave the destination
    /// untouched; the large value never crosses a function return boundary.
    pub(crate) fn initialize_into(
        destination: &mut Option<Self>,
        display: &mut InkplateDriver,
        persisted_settings: PersistedUiSettings,
    ) -> Result<bool, InitError> {
        if destination.is_some() {
            return Err(InitError::AlreadyInitialized);
        }
        let mut coordinator =
            MeditamerCoordinator::new(BASE_PROVIDER_ID, &BASE_SURFACES, HOME_SURFACE_ID)
                .map_err(|_| InitError::ShellConfigurationFailed)?;
        #[cfg(feature = "ui-provider-fixture")]
        let provider_fixture_owner = coordinator
            .register_provider(PROVIDER_FIXTURE_ID, &PROVIDER_FIXTURE_SURFACES)
            .map_err(|_| InitError::ShellConfigurationFailed)?;
        let base_owner = coordinator.shell().active().surface.owner;
        let apps =
            apps::register_apps(base_owner).map_err(|_| InitError::ShellConfigurationFailed)?;
        let home_surface = SurfaceRef::new(base_owner, HOME_SURFACE_ID.0);
        let launcher_surface = SurfaceRef::new(base_owner, LAUNCHER_SURFACE_ID.0);
        let diagnostics_surface = SurfaceRef::new(base_owner, DIAGNOSTICS_SURFACE_ID.0);
        let surfaces = SurfaceRefs {
            home: home_surface,
            launcher: launcher_surface,
            diagnostics: diagnostics_surface,
            ambient_view: SurfaceRef::new(base_owner, AMBIENT_VIEW_SURFACE_ID.0),
            analog_clock: SurfaceRef::new(base_owner, ANALOG_CLOCK_SURFACE_ID.0),
            overlay_toggles: SurfaceRef::new(base_owner, OVERLAY_TOGGLES_SURFACE_ID.0),
            navigation_cue: SurfaceRef::new(base_owner, NAVIGATION_CUE_SURFACE_ID.0),
            sticky_status: SurfaceRef::new(base_owner, STICKY_STATUS_SURFACE_ID.0),
            confirm: SurfaceRef::new(base_owner, CONFIRM_SURFACE_ID.0),
            settings: SurfaceRef::new(base_owner, SETTINGS_SURFACE_ID.0),
            clock_overlay: SurfaceRef::new(base_owner, CLOCK_OVERLAY_SURFACE_ID.0),
            frontlight_calibration: SurfaceRef::new(
                base_owner,
                FRONTLIGHT_CALIBRATION_SURFACE_ID.0,
            ),
            #[cfg(feature = "ui-provider-fixture")]
            provider_fixture: SurfaceRef::new(provider_fixture_owner, PROVIDER_FIXTURE_ROOT_ID.0),
            #[cfg(feature = "ui-provider-fixture")]
            provider_overlay: SurfaceRef::new(
                provider_fixture_owner,
                PROVIDER_FIXTURE_OVERLAY_ID.0,
            ),
        };
        let mut catalogue = build_product_catalogue(surfaces, &apps)?;
        let settings =
            UiSettings::resolve(&catalogue, persisted_settings, &[REFRESH_CONTROL_ENTRY_ID]);
        catalogue.apply_pins(settings.pins());

        prepare_memory_pool()?;
        // CPU0's synchronous LVGL flush copies L8 pixels into the internal I1
        // framebuffer before any panel scan. Neither DMA nor CPU1 retains this
        // buffer. Keep the large drawing scratch beside LVGL's PSRAM arena.
        let draw_buffer = allocate_draw_buffer()?;
        let Ok(session) = RuntimeSession::initialize() else {
            return Err(InitError::AlreadyInitialized);
        };
        let ui_token = session.access_token();
        #[cfg(feature = "ui-initialization-fixture")]
        initialization_fixture::checkpoint(initialization_fixture::Stage::Session)?;

        // RuntimeSession admits only one initialization per boot. LVGL retains
        // this address for that session, including failed bring-up cleanup.
        let Ok(lv_display) =
            session.create_l8_display(WIDTH, HEIGHT, draw_buffer.leak(), BUFFER_BYTES)
        else {
            return Err(InitError::DisplayCreationFailed);
        };

        #[cfg(feature = "ui-initialization-fixture")]
        initialization_fixture::checkpoint(initialization_fixture::Stage::Display)?;
        let Ok(input) = session.create_pointer_input(
            ROTATION_THRESHOLD_RADIANS,
            render::lvgl_adapter::PointerInputCallbacks {
                read: io::pointer_sample,
                gesture: io::record_gesture,
            },
        ) else {
            return Err(InitError::InputCreationFailed);
        };

        let bootstrap_root = ui_token
            .capture_unmanaged_active_screen()
            .map_err(|_| InitError::SurfaceCreationFailed)?
            .ok_or(InitError::SurfaceCreationFailed)?;
        #[cfg(feature = "ui-initialization-fixture")]
        initialization_fixture::checkpoint(initialization_fixture::Stage::Input)?;
        let mut clock_shared = analog_clock::SharedCache::new();
        let mut ambient_shared = ambient_view::SharedCache::new();
        let home = {
            let mut runtime = lvgl_surface_runtime(
                surfaces,
                &apps,
                &catalogue,
                &settings,
                ui_token.clone(),
                0,
                &mut clock_shared,
                &mut ambient_shared,
            );
            ActiveSurface::enter(
                coordinator.shell().active(),
                coordinator.shell().active_instance(),
                &mut runtime,
            )?
        };
        if !home.activate(&ui_token) {
            destroy_initial_surface_or_stop(home, &ui_token);
            return Err(InitError::SurfaceActivationFailed);
        }
        if home.enable().is_err() {
            let _ = bootstrap_root.activate(&ui_token);
            destroy_initial_surface_or_stop(home, &ui_token);
            return Err(InitError::CallbackRouteUnavailable);
        }
        if let Err(error) = bootstrap_root.delete(&ui_token) {
            if let render::lvgl_adapter::UnmanagedScreenDeleteFailure::StillValid(bootstrap_root) =
                error
            {
                let _ = bootstrap_root.activate(&ui_token);
            }
            destroy_initial_surface_or_stop(home, &ui_token);
            return Err(InitError::SurfaceCleanupFailed);
        }
        // `home` is already entered, activated, and enabled above -- LVGL's
        // own `bootstrap_root` swap-and-delete dance has no equivalent in
        // `SurfaceRuntime`'s enter/activate/enable shape, so bring-up stays
        // hand-written here rather than going through
        // `UiCoordinator::bootstrap_screen`.
        coordinator.install_bootstrap_screen(home);

        let now_us = Instant::now().as_micros();
        let mut backend = Self {
            coordinator,
            apps,
            catalogue,
            settings: UiSettingsPersistence::new(settings),
            surfaces,
            input,
            _display: lv_display,
            _runtime_session: session,
            ui_token,
            timer_metrics: TimerServiceMetrics::new(now_us),
            next_timer_due_ms: 0,
            multitouch: LvglMultitouchTracker::default(),
            environment_reading: None,
            environment_state: None,
            #[cfg(feature = "cpu-load")]
            cpu_reading: None,
            clock_removal: ClockRemovalState::None,
            pending_input_transition: None,
            input_transition_rendered: false,
            input_transition_readiness: InputTransitionReadiness::Ready,
            pending_fast_clock: PendingFastClock::None,
            pending_screen_updates: [None; 2],
            frontlight_calibration_initial: 0,
            pending_frontlight_effect: None,
            analog_clock_shared: clock_shared,
            ambient_shared,
            #[cfg(feature = "ui-provider-fixture")]
            provider_fixture_state: ProviderFixtureState::Registered(provider_fixture_owner),
        };
        let overlay_result = backend.install_base_overlay(
            surfaces.navigation_cue,
            OverlayLifetime::Transient,
            1,
            BaseOverlayKind::NavigationCue,
        );
        #[cfg(feature = "ui-initialization-fixture")]
        let overlay_result = overlay_result.and_then(|()| {
            initialization_fixture::checkpoint(initialization_fixture::Stage::NavigationOverlay)
        });
        if let Err(error) = overlay_result {
            destroy_initial_backend_or_stop(backend);
            return Err(error);
        }
        if backend
            .settings
            .current()
            .startup_overlay_enabled(REFRESH_CONTROL_ENTRY_ID)
        {
            let overlay_result = backend.install_base_overlay(
                surfaces.sticky_status,
                OverlayLifetime::Sticky,
                2,
                BaseOverlayKind::RefreshControl,
            );
            #[cfg(feature = "ui-initialization-fixture")]
            let overlay_result = overlay_result.and_then(|()| {
                initialization_fixture::checkpoint(initialization_fixture::Stage::RefreshOverlay)
            });
            if let Err(error) = overlay_result {
                destroy_initial_backend_or_stop(backend);
                return Err(error);
            }
        }
        console::println!("UI_SETTINGS_BOOT base=home state=entered");
        backend.apply_startup_entry();
        backend.log_lifecycle_checkpoint("initialized", 0);
        let rendered = backend
            .run_timers(display, 250)
            .or_else(|| backend.invalidate(display));
        *destination = Some(backend);
        Ok(rendered.is_some())
    }

    /// Requests and admits one base overlay directly, bypassing the queue
    /// (bring-up runs before anything else can contend for the composition).
    /// `kind` only picks the request's [`OverlayInput`] here --
    /// [`LvglOverlayRuntime::enter`] infers the same kind on its own from
    /// `surface`'s identity when this admission's staging actually builds
    /// the widget.
    pub(super) fn install_base_overlay(
        &mut self,
        surface: SurfaceRef,
        lifetime: OverlayLifetime,
        rank: u8,
        kind: BaseOverlayKind,
    ) -> Result<(), InitError> {
        let owned = OwnedCompositionIntent {
            source: self.coordinator.shell().active_instance(),
            intent: CompositionIntent::Request {
                surface,
                input: kind.input(),
                lifetime,
                rank,
            },
        };
        let mut overlay_runtime = self.overlay_runtime();
        let result = self
            .coordinator
            .dispatch_composition(owned, &mut overlay_runtime);
        match result {
            Ok(CompositionDispatchOutcome::Admitted(OverlayAdmission::Active(_))) => {
                self.sync_overlay_visibility_for_active_surface();
                Ok(())
            }
            Ok(CompositionDispatchOutcome::RolledBack(CompositionRollbackReason::Entry)) => {
                Err(InitError::SurfaceCreationFailed)
            }
            _ => Err(InitError::ShellConfigurationFailed),
        }
    }

    fn apply_startup_entry(&mut self) {
        let id = self
            .settings
            .current()
            .startup_entry()
            .unwrap_or(AMBIENT_VIEW_ENTRY_ID);
        let Some(destination) = self
            .catalogue
            .entry(id)
            .and_then(|entry| match entry.action() {
                CatalogueAction::Enter(surface) => Some(surface),
                CatalogueAction::Unavailable(_) => None,
            })
        else {
            console::println!(
                "UI_SETTINGS_STARTUP status=fallback reason=unavailable id={:?}",
                id,
            );
            return;
        };

        for intent in [
            NavIntent::OpenLauncher(self.surfaces.launcher),
            NavIntent::Launch(destination),
        ] {
            if self
                .coordinator
                .queue_intent(OwnedNavIntent {
                    source: self.coordinator.shell().active_instance(),
                    intent,
                })
                .is_err()
            {
                break;
            }
            self.drain_shell_navigation();
            if self.coordinator.navigation_faulted() {
                break;
            }
        }

        if self.coordinator.shell().active().surface == destination
            && self.active_surface_is_renderable()
        {
            console::println!("UI_SETTINGS_STARTUP status=applied id={:?}", id);
            return;
        }
        let _ = self.coordinator.queue_intent(OwnedNavIntent {
            source: self.coordinator.shell().active_instance(),
            intent: NavIntent::Home,
        });
        self.drain_shell_navigation();
        console::println!(
            "UI_SETTINGS_STARTUP status=fallback reason=entry_failed id={:?}",
            id,
        );
    }
}

fn build_product_catalogue(
    surfaces: SurfaceRefs,
    apps: &MeditamerApps,
) -> Result<DefaultCatalogue, InitError> {
    use shell::catalogue::{CatalogueAvailability, CatalogueEntry, EntryId, GlyphRef};

    let mut entries: heapless::Vec<CatalogueEntry, { shell::catalogue::CATALOGUE_CAPACITY }> =
        heapless::Vec::new();
    entries
        .push(CatalogueEntry {
            id: HOME_ENTRY_ID,
            label: c"Meditamer home",
            glyph: GlyphRef(1),
            surface: surfaces.home,
            capabilities: SurfaceCapabilities::AMBIENT,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        })
        .map_err(|_| InitError::ShellConfigurationFailed)?;
    for app in apps.iter() {
        entries
            .push(app.catalogue_entry())
            .map_err(|_| InitError::ShellConfigurationFailed)?;
    }
    entries
        .push(CatalogueEntry {
            id: EntryId::new(NAMESPACE, 5),
            label: c"Refresh control",
            glyph: GlyphRef(5),
            surface: surfaces.sticky_status,
            capabilities: SurfaceCapabilities::OVERLAY,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        })
        .map_err(|_| InitError::ShellConfigurationFailed)?;
    DefaultCatalogue::new(&entries, HOME_ENTRY_ID).map_err(|_| InitError::ShellConfigurationFailed)
}
