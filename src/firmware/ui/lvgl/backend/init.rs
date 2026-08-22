//! Backend bring-up: LVGL init, the draw buffer, and the base surface set.

use super::*;

impl Backend {
    pub(crate) fn initialize(
        display: &mut InkplateDriver,
        persisted_settings: PersistedUiSettings,
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
            ambient_view: SurfaceRef::new(base_owner, AMBIENT_VIEW_SURFACE_ID.0),
            overlay_settings: SurfaceRef::new(base_owner, OVERLAY_SETTINGS_SURFACE_ID.0),
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
        let mut catalogue = build_compiled_catalogue(surfaces)?;
        let settings =
            UiSettings::resolve(&catalogue, persisted_settings, &[REFRESH_CONTROL_ENTRY_ID]);
        catalogue.apply_pins(settings.pins());

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
        let home = ActiveSurface::enter(
            shell.active(),
            shell.active_instance(),
            surfaces,
            &catalogue,
            &settings,
        )?;
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
            catalogue,
            settings: UiSettingsPersistence::new(settings),
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
        if backend
            .settings
            .current()
            .startup_overlay_enabled(REFRESH_CONTROL_ENTRY_ID)
        {
            if let Err(error) = backend.install_base_overlay(
                surfaces.sticky_status,
                OverlayLifetime::Sticky,
                2,
                BaseOverlayKind::RefreshControl,
            ) {
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
        Ok((backend, rendered))
    }

    pub(super) fn install_base_overlay(
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
                self.sync_overlay_visibility_for_active_surface();
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

    fn apply_startup_entry(&mut self) {
        let Some(id) = self.settings.current().startup_entry() else {
            return;
        };
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
                .shell
                .queue_intent(OwnedNavIntent {
                    source: self.shell.active_instance(),
                    intent,
                })
                .is_err()
            {
                break;
            }
            self.drain_shell_navigation();
            if self.navigation_faulted {
                break;
            }
        }

        if self.shell.active().surface == destination && self.active_surface_is_renderable() {
            console::println!("UI_SETTINGS_STARTUP status=applied id={:?}", id);
            return;
        }
        let _ = self.shell.queue_intent(OwnedNavIntent {
            source: self.shell.active_instance(),
            intent: NavIntent::Home,
        });
        self.drain_shell_navigation();
        console::println!(
            "UI_SETTINGS_STARTUP status=fallback reason=entry_failed id={:?}",
            id,
        );
    }
}

fn build_compiled_catalogue(surfaces: SurfaceRefs) -> Result<DefaultCatalogue, InitError> {
    use shell::catalogue::{CatalogueAvailability, CatalogueEntry, EntryId, GlyphRef};

    let entries = [
        CatalogueEntry {
            id: HOME_ENTRY_ID,
            label: c"Meditamer home",
            glyph: GlyphRef(1),
            surface: surfaces.home,
            capabilities: SurfaceCapabilities::AMBIENT,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        },
        CatalogueEntry {
            id: EntryId::new(BASE_NAMESPACE, 2),
            label: c"Gesture diagnostics",
            glyph: GlyphRef(2),
            surface: surfaces.diagnostics,
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        },
        CatalogueEntry {
            id: EntryId::new(BASE_NAMESPACE, 3),
            label: c"Ambient view",
            glyph: GlyphRef(3),
            surface: surfaces.ambient_view,
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 1,
            pin: None,
            availability: CatalogueAvailability::Ready,
        },
        CatalogueEntry {
            id: EntryId::new(BASE_NAMESPACE, 4),
            label: c"Overlay settings",
            glyph: GlyphRef(4),
            surface: surfaces.overlay_settings,
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 2,
            pin: None,
            availability: CatalogueAvailability::Ready,
        },
        CatalogueEntry {
            id: EntryId::new(BASE_NAMESPACE, 5),
            label: c"Refresh control",
            glyph: GlyphRef(5),
            surface: surfaces.sticky_status,
            capabilities: SurfaceCapabilities::OVERLAY,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        },
    ];
    DefaultCatalogue::new(&entries, HOME_ENTRY_ID).map_err(|_| InitError::ShellConfigurationFailed)
}
