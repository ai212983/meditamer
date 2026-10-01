//! Sleep and wake sequencing for the runtime UI loop: the low-level deep-sleep
//! and light-sleep entry points plus the UI/observation/input steps around
//! them. `sleep::enter_sleep`/`sleep::enter_deep_sleep` themselves stay
//! in `crate::sleep` (the actual RTC/deep-sleep register programming); this
//! module is the product-facing sequence around that primitive: closing Home
//! observations, showing a confirmation screen, suspending the shared I2C
//! bus, and (for light sleep) rebuilding KEY recognition on wake.

use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant};
use esp_hal::rtc_cntl::sleep::LowPower;

use medinote::input::CaptureGate;
use medinote::observations::HomeObservations;
use medinote::power::RuntimePowerMode;
use medinote::ui::screen::power as power_screen;
use render::lvgl_adapter::UiAccessToken;
use waveshare_rlcd42::panel::PowerMode;
use waveshare_rlcd42::panel_lvgl::PanelLvglSession;

use crate::observations::{resume_observations, suspend_observations};
use crate::sleep;

use super::{refresh_screen_full_now, MedinoteScreen, RuntimeCoordinator};

pub(crate) fn set_runtime_power(
    current: &mut RuntimePowerMode,
    desired: RuntimePowerMode,
    panel_session: &mut PanelLvglSession,
    reason: &str,
) {
    if *current == desired {
        return;
    }
    // ST7305 regional writes land immediately in both controller modes. The
    // physical device showed visibly reduced contrast in high-power mode, so
    // runtime performance intent must not change the LCD drive waveform.
    // Keep the panel in the normal, high-contrast mode even while the CPU runs
    // the 30 Hz physics loop and sends 10 Hz regional animation updates.
    let panel_mode = PowerMode::Low;
    panel_session.set_power_mode(panel_mode);
    console::println!(
        "RUNTIME_POWER from={:?} to={:?} panel={:?} reason={}",
        *current,
        desired,
        panel_mode,
        reason
    );
    *current = desired;
}

/// Attempts deep sleep. A rejected attempt destroys its temporary screen and
/// returns it only when LVGL reports that cleanup must be retried. The caller
/// owns that returned screen until a later delete succeeds.
pub(crate) async fn try_deep_sleep(
    context: DeepSleepContext<'_>,
) -> Option<power_screen::PowerScreen> {
    set_runtime_power(
        context.runtime_mode,
        RuntimePowerMode::DeepSleep,
        context.panel_session,
        "home-boot-action",
    );
    // The target owns this transient screen. Successful deep sleep powers the
    // chip off; an aborted attempt must return or destroy the live root.
    let sleep_screen = power_screen::create_deep_sleep(context.ui_token);
    if let Some(screen) = &sleep_screen {
        screen.activate(context.ui_token);
    }
    refresh_screen_full_now(context.ui_token, context.panel_session);
    embassy_time::Timer::after(EmbassyDuration::from_millis(250)).await;
    for event in context
        .key_gate
        .cancel(context.button_epoch.elapsed().as_millis())
    {
        super::input::log_button_event(event);
    }
    for event in context
        .boot_gate
        .cancel(context.button_epoch.elapsed().as_millis())
    {
        super::input::log_button_event(event);
    }
    // The panel is flushed, but the I2C bus must also be idle before power-off.
    let quiesced =
        crate::observations::suspend_for_fixture(context.sleep_fixture.map(|f| f.request)).await;
    let storage_quiesced = suspend_storage().await;
    if !quiesced || !storage_quiesced || context.sleep_fixture.is_some_and(|f| !f.eligible()) {
        // Do not power off while a bus transaction may still be active.
        console::println!("DEEP_SLEEP_ABORTED reason=peripherals_not_quiescent_or_expired");
        set_runtime_power(
            context.runtime_mode,
            RuntimePowerMode::Normal,
            context.panel_session,
            "deep-sleep-cleanup-failed",
        );
        return destroy_power_screen(context.ui_token, sleep_screen);
    }
    if let Some(fixture) = context.sleep_fixture {
        fixture.entering();
    }
    sleep::enter_deep_sleep(context.low_power);
}

pub(crate) struct DeepSleepContext<'a> {
    pub(crate) ui_token: &'a UiAccessToken,
    pub(crate) low_power: &'a mut LowPower<'static>,
    pub(crate) runtime_mode: &'a mut RuntimePowerMode,
    pub(crate) key_gate: &'a mut CaptureGate,
    pub(crate) boot_gate: &'a mut CaptureGate,
    pub(crate) button_epoch: EmbassyInstant,
    pub(crate) sleep_fixture: Option<crate::sleep_fixture::PendingSleep>,
    pub(crate) panel_session: &'a mut PanelLvglSession,
}

/// Relabels and reactivates the live Home screen after a sleep transition.
pub(crate) fn reload_active_home(
    ui_token: &UiAccessToken,
    coordinator: &RuntimeCoordinator,
    heading: &core::ffi::CStr,
    caption: &core::ffi::CStr,
    panel_session: &mut PanelLvglSession,
) {
    if let Some(MedinoteScreen::Home(home)) = coordinator.active_screen() {
        home.set_heading(ui_token, heading, caption);
        home.activate(ui_token);
    }
    refresh_screen_full_now(ui_token, panel_session);
}

pub(crate) fn restore_home_after_sleep(
    ui_token: &UiAccessToken,
    observations: &mut HomeObservations,
    coordinator: &RuntimeCoordinator,
    sleep_fixture: Option<crate::sleep_fixture::PendingSleep>,
    panel_session: &mut PanelLvglSession,
) -> EmbassyInstant {
    // Home was already the active shell surface before sleep and stays so
    // across it.
    reload_active_home(ui_token, coordinator, c"Medinote", c"Home", panel_session);
    resume_storage();
    resume_observations(observations, coordinator.shell().active_instance());
    if let Some(fixture) = sleep_fixture {
        fixture.end(if fixture.eligible() {
            "Rejected"
        } else {
            "Expired"
        });
    }
    EmbassyInstant::now()
}

/// What became of one `BootAction::Sleep` attempt.
///
/// Both outcomes preserve ownership of any still-live confirmation screen.
/// The caller performs the post-wake pin reborrow because its lifetime must
/// extend through the runtime task loop.
pub(crate) enum LightSleepOutcome {
    Aborted {
        next_home_refresh: EmbassyInstant,
        cleanup_blocked: Option<power_screen::PowerScreen>,
    },
    Proceed {
        sleep_screen: Option<power_screen::PowerScreen>,
    },
}

/// Shows the confirmation screen and suspends observations before light sleep.
/// An unconfirmed I2C quiescence returns to Home with all live roots retained.
pub(crate) async fn handle_light_sleep(
    ui_token: &UiAccessToken,
    coordinator: &RuntimeCoordinator,
    observations: &mut HomeObservations,
    runtime_mode: &mut RuntimePowerMode,
    panel_session: &mut PanelLvglSession,
) -> LightSleepOutcome {
    set_runtime_power(
        runtime_mode,
        RuntimePowerMode::Sleep,
        panel_session,
        "home-boot-action",
    );
    // The target owns this transient screen and must destroy it after wake.
    let sleep_screen = power_screen::create_sleep(ui_token);
    if let Some(screen) = &sleep_screen {
        screen.activate(ui_token);
    }
    refresh_screen_full_now(ui_token, panel_session);
    embassy_time::Timer::after(EmbassyDuration::from_millis(250)).await;
    let observations_quiesced = suspend_observations().await;
    let storage_quiesced = suspend_storage().await;
    if !observations_quiesced || !storage_quiesced {
        // An unconfirmed-quiescent bus must not enter a CPU-halting sleep
        // mode. Abandon the attempt.
        console::println!("SLEEP_ABORTED reason=peripherals_not_quiescent");
        set_runtime_power(
            runtime_mode,
            RuntimePowerMode::Normal,
            panel_session,
            "sleep-cleanup-failed",
        );
        let cleanup_blocked = destroy_power_screen(ui_token, sleep_screen);
        let next_home_refresh =
            restore_home_after_sleep(ui_token, observations, coordinator, None, panel_session);
        return LightSleepOutcome::Aborted {
            next_home_refresh,
            cleanup_blocked,
        };
    }
    LightSleepOutcome::Proceed { sleep_screen }
}

/// Destroys the sleep screen, reloads Home, and resumes observations.
/// A refused deletion returns the live screen for a later retry.
pub(crate) fn finish_wake(
    ui_token: &UiAccessToken,
    coordinator: &RuntimeCoordinator,
    observations: &mut HomeObservations,
    runtime_mode: &mut RuntimePowerMode,
    sleep_screen: Option<power_screen::PowerScreen>,
    panel_session: &mut PanelLvglSession,
) -> (EmbassyInstant, Option<power_screen::PowerScreen>) {
    set_runtime_power(
        runtime_mode,
        RuntimePowerMode::Normal,
        panel_session,
        "sleep-wake",
    );
    let cleanup_blocked = destroy_power_screen(ui_token, sleep_screen);
    reload_active_home(ui_token, coordinator, c"Sleep", c"Awake", panel_session);
    let next_home_refresh = EmbassyInstant::now();
    resume_storage();
    resume_observations(observations, coordinator.shell().active_instance());
    console::println!("SLEEP_WAKE destination=home");
    (next_home_refresh, cleanup_blocked)
}

/// Retries the target-owned power-screen cleanup slot once. A failed delete
/// returns the live root to the same slot; no second power screen may be
/// created while it remains occupied.
pub(crate) fn retry_blocked_cleanup(
    ui_token: &UiAccessToken,
    cleanup_blocked: &mut Option<power_screen::PowerScreen>,
) {
    let Some(screen) = cleanup_blocked.take() else {
        return;
    };
    *cleanup_blocked = screen.destroy(ui_token).err();
    if cleanup_blocked.is_some() {
        console::println!("POWER_SCREEN_CLEANUP status=blocked");
    } else {
        console::println!("POWER_SCREEN_CLEANUP status=complete");
    }
}

fn destroy_power_screen(
    ui_token: &UiAccessToken,
    screen: Option<power_screen::PowerScreen>,
) -> Option<power_screen::PowerScreen> {
    screen.and_then(|screen| screen.destroy(ui_token).err())
}

async fn suspend_storage() -> bool {
    #[cfg(feature = "wifi-storage")]
    if !crate::net_host::suspend().await {
        return false;
    }
    #[cfg(feature = "sd-storage")]
    {
        crate::storage::suspend().await
    }
    #[cfg(not(feature = "sd-storage"))]
    {
        true
    }
}

fn resume_storage() {
    #[cfg(feature = "sd-storage")]
    crate::storage::resume();
    #[cfg(feature = "wifi-storage")]
    crate::net_host::resume();
}
