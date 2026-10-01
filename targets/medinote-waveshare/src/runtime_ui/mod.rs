//! Medinote's single runtime UI owner: Home, launcher, Hourglass, and explicit
//! power actions.
//!
//! One task, not two: physics runs at a fixed `PHYSICS_HZ` tick every loop
//! iteration unconditionally (never skipped, regardless of how long the
//! previous render took -- "a delayed or skipped render frame changes
//! animation smoothness, not orientation or transferred mass"), and render
//! runs every `hourglass_runtime::RENDER_EVERY_N_TICKS` iterations. A single
//! task avoids sharing `HourglassModel` across a lock for a widget this
//! small; nothing else needs the model concurrently.
//!
//! This module keeps resource assembly and the event pump. [`surfaces`] owns
//! lifecycle adaptation, [`app_router`] owns app dispatch, [`clock_service`]
//! and [`observation_service`] own their independent polling state, [`input`]
//! owns KEY/BOOT recognition, and [`power`] owns sleep/wake sequencing.
//! Runtime-level refresh and timer servicing use the same checked UI token as
//! screen activation; product code never receives raw LVGL object pointers.

mod app_router;
mod clock_service;
mod hourglass_runtime;
mod input;
mod observation_service;
mod power;
mod power_cleanup;
mod surfaces;
mod telemetry;

use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant, Ticker};
use esp_hal::gpio::Input;
use esp_hal::rtc_cntl::sleep::LowPower;
use static_cell::StaticCell;

use hourglass::model::{HourglassModel, PHYSICS_HZ};
use medinote::apps::counter::descriptor as counter_app;
use medinote::apps::descriptor::{MedinoteApps, MedinoteRegistration};
use medinote::apps::hourglass::descriptor as hourglass_app;
#[cfg(feature = "wifi-storage")]
use medinote::apps::network::descriptor as network_app;
use medinote::catalogue;
use medinote::input::{boot_action, BootAction, ButtonId, ButtonTiming, CaptureGate, KeyEdges};
use medinote::power::RuntimePowerMode;
use medinote::ui::screen::launcher;
use render::lvgl_adapter::{
    RuntimeSession, UiAccessToken, UnmanagedScreen, UnmanagedScreenDeleteFailure,
};
use shell::catalogue::CatalogueViewKind;
use shell::types::{SurfaceRef, SurfaceRole};
use waveshare_rlcd42::buttons::Button;
use waveshare_rlcd42::panel::PowerMode;
use waveshare_rlcd42::panel_lvgl::PanelLvglSession;

use app_router::{ActiveAppContext, AppRouter, KeyRoutingContext};
use clock_service::ClockService;
use observation_service::ObservationService;
use power::LightSleepOutcome;
use power_cleanup::PowerScreenCleanup;
#[cfg(feature = "cheertok-controls")]
use surfaces::{install_ble_status_overlay, MedinoteOverlay};
use surfaces::{
    navigate, MedinoteOverlayRuntime, MedinoteScreen, MedinoteScreenRuntime, RuntimeCoordinator,
    BASE_SURFACES,
};
use telemetry::RuntimeTelemetry;

use crate::SharedI2c;

fn monotonic_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

// BOOT currently exposes Deep Sleep directly while the runtime power-mode UI
// is being validated on hardware.
const BOOT_ENTERS_DEEP_SLEEP: bool = true;

fn service_lvgl(ui_token: &UiAccessToken, tick_ms: u32) {
    let Ok(mut idle_ms) = ui_token.run_timer_handler(tick_ms) else {
        return;
    };
    for _ in 1..8 {
        if idle_ms > 0 {
            break;
        }
        let Ok(next_idle_ms) = ui_token.run_timer_handler(0) else {
            break;
        };
        idle_ms = next_idle_ms;
    }
}

/// Render every currently invalidated object immediately, then push the
/// completed framebuffer to the reflective panel before clocks can stop.
fn refresh_screen_full_now(ui_token: &UiAccessToken, panel_session: &mut PanelLvglSession) {
    let _ = ui_token.refresh_default_display();
    panel_session.force_full_refresh();
}

fn bootstrap_runtime_screen(
    ui_token: &UiAccessToken,
    bootstrap_root: UnmanagedScreen,
    coordinator: &mut RuntimeCoordinator,
    screen_runtime: &mut MedinoteScreenRuntime<'_>,
) {
    if coordinator.bootstrap_screen(screen_runtime).is_err() {
        let _ = bootstrap_root.activate(ui_token);
        panic!("Medinote's screen runtime failed to enter Home");
    }
    if let Err(error) = bootstrap_root.delete(ui_token) {
        if let UnmanagedScreenDeleteFailure::StillValid(bootstrap_root) = error {
            let _ = bootstrap_root.activate(ui_token);
        }
        panic!("Medinote failed to retire LVGL's boot screen");
    }
}

pub(crate) struct RuntimeUiResources {
    pub(crate) runtime_session: RuntimeSession,
    pub(crate) ui_token: UiAccessToken,
    pub(crate) bootstrap_root: UnmanagedScreen,
    pub(crate) rtc_i2c: SharedI2c,
    pub(crate) low_power: LowPower<'static>,
    pub(crate) key_pin: esp_hal::peripherals::GPIO18<'static>,
    pub(crate) boot_pin: esp_hal::peripherals::GPIO0<'static>,
    pub(crate) panel_session: PanelLvglSession,
}

#[embassy_executor::task]
pub async fn runtime_ui_task(resources: RuntimeUiResources) {
    let RuntimeUiResources {
        runtime_session,
        ui_token,
        bootstrap_root,
        rtc_i2c,
        mut low_power,
        mut key_pin,
        boot_pin,
        mut panel_session,
    } = resources;
    let _runtime_session = runtime_session;
    let mut coordinator = RuntimeCoordinator::new(
        catalogue::BASE_PROVIDER_ID,
        BASE_SURFACES,
        catalogue::HOME_SURFACE_ID,
    )
    .expect("base shell");
    let home_surface = coordinator.shell().active().surface;
    let launcher_surface = SurfaceRef {
        owner: home_surface.owner,
        id: catalogue::LAUNCHER_SURFACE_ID,
    };
    #[cfg(feature = "cheertok-controls")]
    let cheertok_status_surface = SurfaceRef {
        owner: home_surface.owner,
        id: catalogue::CHEERTOK_STATUS_SURFACE_ID,
    };
    let settings_surface = SurfaceRef {
        owner: home_surface.owner,
        id: catalogue::SETTINGS_SURFACE_ID,
    };
    let mut overlay_runtime = MedinoteOverlayRuntime {
        ui_token: ui_token.clone(),
        settings_surface,
    };
    // Pair each descriptor with its shell-issued provider token, then
    // compile the immutable catalogue once from the one container.
    let hourglass_owner = coordinator
        .register_provider(
            hourglass_app::PROVIDER_ID,
            hourglass_app::DESCRIPTOR.surfaces,
        )
        .expect("hourglass provider");
    let counter_owner = coordinator
        .register_provider(counter_app::PROVIDER_ID, counter_app::DESCRIPTOR.surfaces)
        .expect("counter provider");
    #[cfg(feature = "wifi-storage")]
    let network_owner = coordinator
        .register_provider(network_app::PROVIDER_ID, network_app::DESCRIPTOR.surfaces)
        .expect("network provider");
    let mut apps = MedinoteApps::new();
    apps.register(MedinoteRegistration::new(
        &hourglass_app::DESCRIPTOR,
        hourglass_owner,
    ))
    .expect("hourglass registers");
    apps.register(MedinoteRegistration::new(
        &counter_app::DESCRIPTOR,
        counter_owner,
    ))
    .expect("counter registers");
    #[cfg(feature = "wifi-storage")]
    apps.register(MedinoteRegistration::new(
        &network_app::DESCRIPTOR,
        network_owner,
    ))
    .expect("network registers");
    let product_catalogue = catalogue::build(home_surface, &apps).expect("Medinote catalogue");
    // Preserve the product catalogue's launcher order for the target lifetime.
    let mut launcher_labels: heapless::Vec<&'static core::ffi::CStr, { launcher::MAX_ENTRIES }> =
        heapless::Vec::new();
    for entry in product_catalogue
        .view(CatalogueViewKind::Launcher)
        .entries()
    {
        let _ = launcher_labels.push(entry.label);
    }
    let mut screen_runtime = MedinoteScreenRuntime {
        ui_token: ui_token.clone(),
        counter_state: medinote::apps::counter::CounterState::new(),
        apps: &apps,
        launcher_labels: &launcher_labels,
    };

    let key_input = Input::new(
        key_pin.reborrow(),
        waveshare_rlcd42::buttons::button_input_config(),
    );
    let boot_input = Input::new(boot_pin, waveshare_rlcd42::buttons::button_input_config());
    let mut key_button = Button::new(key_input);
    let boot_button = Button::new(boot_input);
    let mut key_gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());
    let mut boot_gate = CaptureGate::new(ButtonId::Boot, ButtonTiming::default());
    let button_epoch = EmbassyInstant::now();

    let mut clock_service = ClockService::new(rtc_i2c);

    bootstrap_runtime_screen(
        &ui_token,
        bootstrap_root,
        &mut coordinator,
        &mut screen_runtime,
    );
    #[cfg(feature = "cheertok-controls")]
    let ble_status_token = install_ble_status_overlay(
        &mut coordinator,
        &mut overlay_runtime,
        cheertok_status_surface,
    );
    service_lvgl(&ui_token, 1);
    panel_session.set_power_mode(PowerMode::Low);
    console::println!(
        "UI_SURFACE role={:?} key_gpio={} boot_gpio={}",
        coordinator.shell().active().role,
        waveshare_rlcd42::buttons::KEY_GPIO_DESCRIPTION,
        waveshare_rlcd42::buttons::BOOT_GPIO_DESCRIPTION,
    );

    let tick_period = EmbassyDuration::from_hz(PHYSICS_HZ as u64);
    let mut ticker = Ticker::every(tick_period);
    // Keep the paper model's two occupancy masks out of the task stack. The
    // explicit initializer constructs them directly in this static owner.
    static HOURGLASS_MODEL: StaticCell<HourglassModel> = StaticCell::new();
    let hourglass_model = HourglassModel::init_in_place(HOURGLASS_MODEL.uninit());
    let mut app_router = AppRouter::new(
        ui_token.clone(),
        &product_catalogue,
        &apps,
        &launcher_labels,
        launcher_surface,
        hourglass_runtime::HourglassRuntime::new(hourglass_model),
    );
    let mut runtime_mode = RuntimePowerMode::Normal;
    // Host/controller startup can block executor polls. Publish sensor demand
    // and start RTC deadlines only once that startup has completed or failed.
    #[cfg(feature = "cheertok-controls")]
    crate::cheertok::wait_for_startup().await;
    let mut observation_service = ObservationService::new(settings_surface, &coordinator);
    let mut power_cleanup = PowerScreenCleanup::new();
    let mut telemetry = RuntimeTelemetry::new();

    loop {
        // Retry every explicit owner before creating or dispatching UI roots.
        render::lvgl_adapter::retry_parked_widgets(&ui_token);
        coordinator.retry_blocked_cleanup(&mut screen_runtime, &mut overlay_runtime);
        power_cleanup.retry(&ui_token);

        let sleep_fixture = observation_service.begin_sleep_fixture(
            &mut coordinator,
            &mut screen_runtime,
            &mut overlay_runtime,
        );
        #[cfg(feature = "cheertok-controls")]
        {
            let state = crate::cheertok::ui_state();
            let updated = coordinator
                .live_overlay_mut(ble_status_token, &overlay_runtime)
                .is_some_and(|overlay| {
                    let MedinoteOverlay::BleStatus { widget, .. } = overlay else {
                        return false;
                    };
                    widget.update(&ui_token, state)
                });
            if updated {
                console::println!("CHEERTOK_UI state={:?}", state);
                service_lvgl(&ui_token, 1);
            }
        }
        let now_ms = button_epoch.elapsed().as_millis();
        // A live modal (Settings today) captures physical input the same way
        // Meditamer's touchscreen exclusive-capture does -- Medinote has no
        // LVGL-level input-capture concept of its own (KEY/BOOT are sampled
        // and branched on directly here, not through LVGL click targets), so
        // this runtime loop is its own equivalent gate.
        let modal_active = coordinator.shell().active_modal().is_some();
        let button_tick = input::sample_tick(input::ButtonSample {
            key_gate: &mut key_gate,
            boot_gate: &mut boot_gate,
            key_button: &key_button,
            boot_button: &boot_button,
            surface_instance: coordinator.shell().active_instance(),
            modal_active,
            now_ms,
            now_us: monotonic_micros(),
        });
        let key_pressed = button_tick.key_pressed;
        let key_pressed_at_us = button_tick.key_pressed_at_us;
        let key_edges = KeyEdges {
            pressed: key_pressed,
            clicked: button_tick.key_clicked,
            long_pressed: button_tick.key_long_pressed,
        };
        let boot_pressed = button_tick.boot_pressed;
        let mut key_consumed = false;
        let mut screen_dirty = false;
        let rotation_inputs = AppRouter::collect_rotation_inputs();

        // A headless fixture may request sleep independently of physical input.
        let sleep_requested = (boot_pressed && !modal_active) || sleep_fixture.is_some();
        if !power_cleanup.admit_sleep(sleep_requested, sleep_fixture.as_ref()) {
            continue;
        }
        if sleep_requested {
            // Fixtures force deep sleep after their admission path returns Home.
            let role = coordinator.shell().active().role;
            match boot_action(role, BOOT_ENTERS_DEEP_SLEEP || sleep_fixture.is_some()) {
                BootAction::DeepSleep => {
                    // Close Home ownership before its widgets are replaced.
                    observation_service.deactivate();
                    let cleanup_blocked = power::try_deep_sleep(power::DeepSleepContext {
                        ui_token: &ui_token,
                        low_power: &mut low_power,
                        runtime_mode: &mut runtime_mode,
                        key_gate: &mut key_gate,
                        boot_gate: &mut boot_gate,
                        button_epoch,
                        sleep_fixture,
                        panel_session: &mut panel_session,
                    })
                    .await;
                    power_cleanup.retain(cleanup_blocked);
                    let refreshed_at = power::restore_home_after_sleep(
                        &ui_token,
                        observation_service.observations_mut(),
                        &coordinator,
                        sleep_fixture,
                        &mut panel_session,
                    );
                    clock_service.schedule_at(refreshed_at);
                    continue;
                }
                BootAction::Sleep => {
                    // Close Home ownership before its widgets are replaced.
                    observation_service.deactivate();
                    let sleep_screen = match power::handle_light_sleep(
                        &ui_token,
                        &coordinator,
                        observation_service.observations_mut(),
                        &mut runtime_mode,
                        &mut panel_session,
                    )
                    .await
                    {
                        LightSleepOutcome::Aborted {
                            next_home_refresh: refreshed_at,
                            cleanup_blocked,
                        } => {
                            power_cleanup.retain(cleanup_blocked);
                            clock_service.schedule_at(refreshed_at);
                            continue;
                        }
                        LightSleepOutcome::Proceed { sleep_screen } => sleep_screen,
                    };
                    // Preserve input recognizers until sleep is certain to proceed.
                    let _ = key_button.into_inner();
                    for event in key_gate.cancel(button_epoch.elapsed().as_millis()) {
                        input::log_button_event(event);
                    }
                    for event in boot_gate.cancel(button_epoch.elapsed().as_millis()) {
                        input::log_button_event(event);
                    }
                    // Light sleep retains RAM, so the same providers resume after wake.
                    crate::sleep::enter_sleep(&mut low_power, &mut key_pin);
                    // This task must own the reborrow for as long as the Button lives.
                    let key_input = Input::new(
                        key_pin.reborrow(),
                        waveshare_rlcd42::buttons::button_input_config(),
                    );
                    // A KEY wake must not immediately become an Apps action.
                    // Wait for the physical release, then start a fresh edge
                    // detector for ordinary runtime input.
                    while key_input.is_low() {
                        embassy_time::Timer::after(EmbassyDuration::from_millis(10)).await;
                    }
                    key_button = Button::new(key_input);
                    key_gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());
                    let (refreshed_at, cleanup_blocked) = power::finish_wake(
                        &ui_token,
                        &coordinator,
                        observation_service.observations_mut(),
                        &mut runtime_mode,
                        sleep_screen,
                        &mut panel_session,
                    );
                    power_cleanup.retain(cleanup_blocked);
                    clock_service.schedule_at(refreshed_at);
                    ticker = Ticker::every(tick_period);
                }
                BootAction::Navigate(intent) => {
                    let committed = navigate(
                        &mut coordinator,
                        observation_service.observations_mut(),
                        intent,
                        &mut screen_runtime,
                        &mut overlay_runtime,
                    );
                    screen_dirty |= committed;
                    power::set_runtime_power(
                        &mut runtime_mode,
                        RuntimePowerMode::Normal,
                        &mut panel_session,
                        "navigation-back",
                    );
                    if coordinator.shell().active().role == SurfaceRole::Ambient {
                        clock_service.schedule_at(EmbassyInstant::now());
                    }
                    console::println!("UI_SURFACE role={:?}", coordinator.shell().active().role);
                }
            }
        } else if (key_edges.pressed || key_edges.clicked || key_edges.long_pressed)
            && !modal_active
        {
            let outcome = app_router.route_key(
                key_edges,
                KeyRoutingContext {
                    coordinator: &mut coordinator,
                    observations: observation_service.observations_mut(),
                    screen_runtime: &mut screen_runtime,
                    overlay_runtime: &mut overlay_runtime,
                },
            );
            key_consumed = outcome.consumed;
            screen_dirty |= outcome.screen_dirty;
        }

        screen_dirty |= clock_service.poll(&ui_token, &coordinator).await;
        screen_dirty |= observation_service.poll(
            &ui_token,
            &mut coordinator,
            &mut overlay_runtime,
            clock_service.jtag_owned(),
        );
        if screen_dirty {
            let _ = ui_token.refresh_default_display();
        }

        app_router.tick_active(ActiveAppContext {
            coordinator: &coordinator,
            screen_runtime: &mut screen_runtime,
            runtime_mode: &mut runtime_mode,
            panel_session: &mut panel_session,
            key_consumed,
            key_pressed_at_us,
            rotation_inputs,
            telemetry: &mut telemetry,
        });
        telemetry.poll(&ui_token);

        ticker.next().await;
    }
}
