//! Target observation effects: Watch transport, entry admission, two-provider
//! sleep quiescence and Home presentation. Product policy stays in HomeObservations.

use crate::{battery, environment};
use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant};
use medinote::observations::fixture::{
    ConsoleCommand, ConsoleInput, FixtureProvider, ParsedCommand, RX_BYTES_PER_POLL,
};
use medinote::observations::HomeObservations;
use medinote::presentation::{format_battery, format_reading};
use medinote::ui::screen::home::Home;
use observation::fixture::{FixtureRequest, FixtureSample, FixtureState};
use observation::ingress::RequestAdmissionError;
use observation::observe_now::ObserveNowRequest;
use observation::policy::Health;
use observation::time::Instant as ObsInstant;
use render::lvgl_adapter::UiAccessToken;
use shell::types::SurfaceInstanceToken;

/// What the console byte stream asked the Settings overlay to do this poll,
/// if anything -- Medinote has no touchscreen, so this is the only way to
/// open or close it (see `ConsoleCommand::UiSettings`/
/// `UiClose`). `runtime_ui_task` owns actually dispatching either request
/// through the coordinator; this module only surfaces it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsOverlayRequest {
    None,
    Open,
    Close,
}

pub(crate) fn environment_now() -> ObsInstant {
    ObsInstant(EmbassyInstant::now().as_millis())
}

/// Publish the product adapter's demand using the existing target watches.
pub(crate) fn sync_observation_demand(observations: &HomeObservations) {
    let now = environment_now();
    environment::ENVIRONMENT_DEMAND.publish(observations.environment_demand(now));
    battery::BATTERY_DEMAND.publish(observations.battery_demand(now));
}

pub(crate) fn deactivate_observations(observations: &mut HomeObservations) {
    observations.deactivate();
    sync_observation_demand(observations);
}

/// Close both ingresses before awaiting either acknowledgement. The two actors
/// share the existing sleep deadline; only both current cleanups permit sleep.
pub(crate) async fn suspend_observations() -> bool {
    suspend_for_fixture(None).await
}

pub(crate) async fn suspend_for_fixture(
    fixture: Option<medinote::observations::sleep_fixture::SleepRequest>,
) -> bool {
    use medinote::observations::sleep_fixture::SleepMode;
    const SUSPEND_ACK_TIMEOUT: EmbassyDuration = EmbassyDuration::from_secs(2);
    let deadline = EmbassyInstant::now() + SUSPEND_ACK_TIMEOUT;
    let environment_wait = environment::ENVIRONMENT_CONTROL.suspend(
        &environment::ENVIRONMENT_REQUESTS,
        embassy_time::Timer::at(deadline),
        fixture.is_some_and(|f| f.mode == SleepMode::RejectEnvironment),
    );
    let battery_wait = battery::BATTERY_CONTROL.suspend(
        &battery::BATTERY_REQUESTS,
        embassy_time::Timer::at(deadline),
        fixture.is_some_and(|f| f.mode == SleepMode::RejectBattery),
    );
    let (environment_outcome, battery_outcome) =
        embassy_futures::join::join(environment_wait, battery_wait).await;
    console::println!("ENVIRONMENT_SUSPEND outcome={:?}", environment_outcome);
    console::println!("BATTERY_SUSPEND outcome={:?}", battery_outcome);
    if let Some(fixture) = fixture {
        let (environment, _) = environment::ENVIRONMENT_CONTROL.snapshot();
        let (battery, _) = battery::BATTERY_CONTROL.snapshot();
        console::println!("OBSSLEEP CONTROL id={} environment_id={} environment_ack={:?} battery_id={} battery_ack={:?}", fixture.id, environment.id(), environment_outcome, battery.id(), battery_outcome);
    }
    environment_outcome.permits_sleep() && battery_outcome.permits_sleep()
}

/// Both resume intents precede Home renewal. Entry admission retries while a
/// provider has not consumed its current resume and reopened ingress yet.
pub(crate) fn resume_observations(
    observations: &mut HomeObservations,
    instance: SurfaceInstanceToken,
) {
    environment::ENVIRONMENT_CONTROL.request_resume();
    battery::BATTERY_CONTROL.request_resume();
    if observations.rebuilt(instance, environment_now()).is_err() {
        console::println!("HOME_OBSERVATIONS_DISABLED reason=owner_generation_exhausted");
    }
    sync_observation_demand(observations);
}

/// Non-blocking product delivery; target code owns only Watch/LVGL effects.
/// `home` is `None` whenever Home is not the active surface (Launcher,
/// Hourglass) -- polling still runs so admission/fixture bookkeeping stays
/// current, but there is no label to rewrite.
fn poll_environment(
    observations: &mut HomeObservations,
    request: Option<FixtureRequest>,
    ui_token: &UiAccessToken,
    home: Option<&Home>,
) -> bool {
    let now = environment_now();
    let state = environment::ENVIRONMENT_STATE.try_get();
    if let Some(event) = observations.admit_environment(
        now,
        state,
        environment::ENVIRONMENT_REQUESTS.close_generation(),
        |request, admitted_at| environment::ENVIRONMENT_REQUESTS.try_admit(request, admitted_at),
    ) {
        console::println!(
            "HOME_ENTRY_REQUEST provider=ENVIRONMENT outcome={:?}",
            event
        );
    }
    service_fixture(
        observations.environment_fixture(),
        now,
        state,
        environment::ENVIRONMENT_REQUESTS.close_generation(),
        request,
        |request, at| environment::ENVIRONMENT_REQUESTS.try_admit(request, at),
    );
    let Some(key) = observations.environment_key() else {
        return false;
    };
    let Some(state) = state else { return false };
    let Some(delivery) = observations.poll_environment(key, state, now) else {
        return false;
    };
    let mut changed = false;
    if let Some(health) = delivery.health {
        let status = match health {
            Health::Ok => c"Normal mode",
            Health::Degraded | Health::Failed => c"sensor error",
            Health::Unknown => c"sensor pending",
        };
        if let Some(home) = home {
            home.set_status(ui_token, status);
        }
        changed = true;
    }
    if let Some(sample) = delivery.sample {
        let mut reading_text = [0u8; 32];
        let len = format_reading(
            &mut reading_text,
            sample.temperature_millicelsius,
            sample.humidity_millipercent,
        );
        if let Ok(text) = core::ffi::CStr::from_bytes_with_nul(&reading_text[..len]) {
            if let Some(home) = home {
                home.set_reading(ui_token, text);
            }
        }
        console::println!(
            "HOME_SAMPLE t_mC={} rh_mpermil={} generation={} revision={} last_sample_at_ms={:?} last_sample_revision={:?} last_attempt_at_ms={:?}",
            sample.temperature_millicelsius,
            sample.humidity_millipercent,
            state.generation.0,
            state.revision.0,
            state.last_sample_at.map(|at| at.0),
            state.last_sample_revision.map(|revision| revision.0),
            state.last_attempt_at.map(|at| at.0)
        );
        changed = true;
    }
    changed
}

fn poll_battery(
    observations: &mut HomeObservations,
    request: Option<FixtureRequest>,
    ui_token: &UiAccessToken,
    home: Option<&Home>,
) -> bool {
    let now = environment_now();
    let state = battery::BATTERY_STATE.try_get();
    if let Some(event) = observations.admit_battery(
        now,
        state,
        battery::BATTERY_REQUESTS.close_generation(),
        |request, admitted_at| battery::BATTERY_REQUESTS.try_admit(request, admitted_at),
    ) {
        console::println!("HOME_ENTRY_REQUEST provider=BATTERY outcome={:?}", event);
    }
    service_fixture(
        observations.battery_fixture(),
        now,
        state,
        battery::BATTERY_REQUESTS.close_generation(),
        request,
        |request, at| battery::BATTERY_REQUESTS.try_admit(request, at),
    );
    let Some(key) = observations.battery_key() else {
        return false;
    };
    let Some(state) = state else { return false };
    let Some(delivery) = observations.poll_battery(key, state, now) else {
        return false;
    };
    if let Some(health) = delivery.health {
        console::println!(
            "HOME_BATTERY_HEALTH health={:?} generation={} revision={}",
            health,
            state.generation.0,
            state.revision.0
        );
    }
    // Battery has no health widget. A health-only result logs promptly and
    // retains the previous reading without scheduling a redundant repaint.
    let Some(sample) = delivery.sample else {
        return false;
    };
    let mut battery_text = [0u8; 24];
    let len = format_battery(&mut battery_text, sample.percent, sample.millivolts);
    if let Ok(text) = core::ffi::CStr::from_bytes_with_nul(&battery_text[..len]) {
        if let Some(home) = home {
            home.set_battery(ui_token, text);
        }
    }
    console::println!(
        "HOME_BATTERY mv={} pct={} health={:?} generation={} revision={} last_sample_at_ms={:?} last_sample_revision={:?} last_attempt_at_ms={:?}",
        sample.millivolts,
        sample.percent,
        state.health,
        state.generation.0,
        state.revision.0,
        state.last_sample_at.map(|at| at.0),
        state.last_sample_revision.map(|revision| revision.0),
        state.last_attempt_at.map(|at| at.0)
    );
    true
}

/// Called on every UI tick, including Launcher/Hourglass. There is exactly
/// one JTAG-RX owner at a time: while `jtag_owned_elsewhere` is set (a
/// wall-clock boot-sync session is polling for its reply, `wall_clock::
/// ClockSession`), this function leaves the RX FIFO untouched rather than
/// racing that session for bytes -- splitting one logical line between two
/// readers would corrupt both. Once the session settles, ownership reverts
/// here automatically; no other change is needed at either call site.
pub(crate) fn poll_observations(
    context: PollObservationsContext<'_>,
) -> (
    bool,
    Option<medinote::observations::sleep_fixture::SleepRequest>,
    SettingsOverlayRequest,
) {
    let mut command = None;
    if !context.jtag_owned_elsewhere {
        for _ in 0..RX_BYTES_PER_POLL {
            let Some(byte) = waveshare_rlcd42::jtag_rx::try_read_byte() else {
                break;
            };
            if let Some(parsed) = context.input.push_with_handler(byte, handle_target_command) {
                command = Some(parsed);
                break;
            }
        }
    }
    let mut settings_request = SettingsOverlayRequest::None;
    let request = match command {
        Some(ConsoleCommand::Sleep(request)) => {
            return (false, Some(request), SettingsOverlayRequest::None)
        }
        Some(ConsoleCommand::SleepStale(id)) => {
            console::println!("OBSSLEEP END id={} status=StaleId", id);
            None
        }
        Some(ConsoleCommand::Ping) => {
            console::println!("PONG");
            None
        }
        Some(ConsoleCommand::UiSettings) => {
            settings_request = SettingsOverlayRequest::Open;
            None
        }
        Some(ConsoleCommand::UiClose) => {
            settings_request = SettingsOverlayRequest::Close;
            None
        }
        Some(ConsoleCommand::Fixture(command)) => {
            let request = match command.provider {
                FixtureProvider::Shtc3 => {
                    route_command::<medinote::observations::EnvironmentStateSnapshot>(
                        command,
                        &environment::ENVIRONMENT_DEMAND,
                        environment::ENVIRONMENT_REQUESTS.close_generation(),
                        context.observations.environment_fixture().is_pending(),
                    )
                }
                FixtureProvider::Adc => {
                    route_command::<medinote::observations::BatteryStateSnapshot>(
                        command,
                        &battery::BATTERY_DEMAND,
                        battery::BATTERY_REQUESTS.close_generation(),
                        context.observations.battery_fixture().is_pending(),
                    )
                }
            };
            request.map(|request| (command.provider, request))
        }
        Some(ConsoleCommand::Rejected(command, status)) => {
            console::println!(
                "{} RESULT id={} provider={} status={:?}",
                command.mode.prefix(),
                command.id,
                command.provider.id().0,
                status
            );
            None
        }
        Some(ConsoleCommand::Invalid) => {
            console::println!("OBSFIX INVALID");
            None
        }
        None => None,
    };
    let battery = poll_battery(
        context.observations,
        request.filter(|r| r.0 == FixtureProvider::Adc).map(|r| r.1),
        context.ui_token,
        context.home,
    );
    // Do not short-circuit: both providers get serviced on every iteration.
    let environment = poll_environment(
        context.observations,
        request
            .filter(|r| r.0 == FixtureProvider::Shtc3)
            .map(|r| r.1),
        context.ui_token,
        context.home,
    );
    (battery || environment, None, settings_request)
}

pub(crate) struct PollObservationsContext<'a> {
    pub(crate) observations: &'a mut HomeObservations,
    pub(crate) input: &'a mut ConsoleInput,
    pub(crate) ui_token: &'a UiAccessToken,
    pub(crate) home: Option<&'a Home>,
    pub(crate) jtag_owned_elsewhere: bool,
}

fn service_fixture<S: FixtureSample>(
    fixture: &mut FixtureState<S>,
    now: ObsInstant,
    state: Option<S>,
    close_generation: u32,
    request: Option<FixtureRequest>,
    admit: impl FnOnce(ObserveNowRequest<S::Fields>, ObsInstant) -> Result<(), RequestAdmissionError>,
) {
    if let Some(result) = fixture.poll(now, state, close_generation) {
        console::println!("{}", result);
    }
    if let Some(request) = request {
        if let Some(result) = fixture.begin(request, now, state, close_generation, admit) {
            console::println!("{}", result);
        } else {
            console::println!(
                "OBSFIX ADMITTED id={} provider={}",
                request.id,
                S::PROVIDER.0
            );
        }
    }
}

fn route_command<S: FixtureSample>(
    command: ParsedCommand,
    control: &observation::periodic::DemandControl<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        S::Fields,
        2,
    >,
    epoch: u32,
    once_pending: bool,
) -> Option<FixtureRequest> {
    let now = environment_now();
    match control.command(
        command.mode,
        command.at(now),
        S::fields(),
        now,
        epoch,
        once_pending,
    ) {
        Ok(Some(request)) => Some(request),
        Ok(None) => {
            console::println!(
                "OBSPER QUEUED id={} provider={}",
                command.id,
                command.provider.id().0
            );
            None
        }
        Err(status) => {
            console::println!(
                "{} RESULT id={} provider={} status={:?}",
                command.mode.prefix(),
                command.id,
                command.provider.id().0,
                status
            );
            None
        }
    }
}

fn handle_target_command(line: &[u8]) -> bool {
    #[cfg(feature = "wifi-storage")]
    if crate::net_commands::handle_line(line) {
        return true;
    }
    #[cfg(feature = "sd-storage")]
    if line.starts_with(b"SDFAT") {
        match sdcard::command_parser::parse(line) {
            Some(command) => {
                if crate::storage::try_submit(command).is_err() {
                    console::println!("SDFAT BUSY");
                }
            }
            None => console::println!("SDFAT INVALID"),
        }
        return true;
    }
    let _ = line;
    false
}
