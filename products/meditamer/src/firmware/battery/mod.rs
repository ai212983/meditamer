//! BQ27441 acquisition owns the shared-expander wake and fuel-gauge read.
//! Product authority owns demand, entry requests and IMU trace delivery. The
//! provider publishes latest state and participates in correlated panel-bus
//! suspension independently of the bounded observation request capacity.

mod config;
mod driver;
mod runtime;
mod types;

pub(crate) use config::{BATTERY_DEMAND, BATTERY_REQUESTS, BATTERY_STATE};
pub(crate) use types::{
    BatteryFields, BatteryStateSnapshot, BATTERY_PROVIDER_ID, FIELDS, SUBSCRIPTION_CAPACITY,
};

use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant, Timer};
use inkplate_tempera::expander::PcalExpander;
use observation::runtime::StepOutcome;
use observation::time::Instant as ObsInstant;

use crate::firmware::bounded_control::SuspendAck;
use crate::firmware::types::{PanelI2cDevice, SharedI2cDevice};
use config::CONTROL;
use driver::{BatteryDiagnostics, BatteryError, BqDriver, FuelGaugeWake};
use runtime::BatteryRuntime;

fn now_ticks() -> ObsInstant {
    ObsInstant(EmbassyInstant::now().as_millis())
}

const CONTROL_TIMEOUT: EmbassyDuration = EmbassyDuration::from_secs(2);

pub async fn suspend_battery_acquisition() -> SuspendAck {
    // Closing before any await also protects timed-out or dropped callers.
    BATTERY_REQUESTS.close();
    let Some(request) = CONTROL.request_suspend() else {
        return SuspendAck::Exhausted;
    };
    CONTROL
        .wait_suspended(request, Timer::after(CONTROL_TIMEOUT))
        .await
}

pub async fn resume_battery_acquisition() -> bool {
    CONTROL.resume(false, Timer::after(CONTROL_TIMEOUT)).await
}

pub fn try_request_battery_acquisition_resume() -> bool {
    CONTROL.request_resume(false);
    true
}

// --- Production diagnostics sink and wake operation ---------------------

/// The real, target-only [`BatteryDiagnostics`] impl -- the only place in
/// this module that touches `console`, so `driver.rs` itself never needs
/// to.
struct ConsoleDiagnostics;

impl<E: core::fmt::Debug> BatteryDiagnostics<E> for ConsoleDiagnostics {
    fn acquire_failed(&mut self, error: &BatteryError<E>) {
        console::println!("BATTERY_ACQUIRE_FAILED error={:?}", error);
    }
}

/// The real, target-only [`FuelGaugeWake`] impl -- the only place in this
/// module that names `PcalExpander` directly, so `driver.rs` itself never
/// needs to (see that module's own doc comment for why: `expander` is
/// `#[cfg(target_os = "none")]`, unreachable from a host build).
struct SharedExpanderWake {
    expander: &'static Mutex<CriticalSectionRawMutex, PcalExpander<PanelI2cDevice>>,
}

impl FuelGaugeWake for SharedExpanderWake {
    // `PcalExpander::wake_fuel_gauge`'s `Result<(), I2C::Error>` resolves
    // through this crate's own `type Result<T, E> = core::result::Result<T,
    // InkplateHalError<E>>` alias, so the error it actually produces is
    // wrapped, not the raw `embedded_hal_async` error.
    type Error = inkplate_tempera::InkplateHalError<
        <PanelI2cDevice as embedded_hal_async::i2c::ErrorType>::Error,
    >;

    async fn wake_fuel_gauge(&mut self) -> Result<(), Self::Error> {
        self.expander.lock().await.wake_fuel_gauge().await
    }
}

#[embassy_executor::task]
pub async fn battery_acquisition_task(
    i2c: SharedI2cDevice,
    expander: &'static Mutex<CriticalSectionRawMutex, PcalExpander<PanelI2cDevice>>,
) {
    let driver = BqDriver::new(i2c, SharedExpanderWake { expander }, ConsoleDiagnostics);
    let mut runtime = BatteryRuntime::new(driver, &BATTERY_REQUESTS, &CONTROL);
    let state_sender = BATTERY_STATE.sender();
    loop {
        if runtime.is_stopped() {
            let request = CONTROL.receive().await;
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), runtime.is_stopped()) {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    BATTERY_DEMAND.live(),
                );
            }
            runtime.apply_control(request, now_ticks).await;
            continue;
        }
        if let Some(request) = CONTROL.try_receive() {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), runtime.is_stopped()) {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    BATTERY_DEMAND.live(),
                );
            }
            runtime.apply_control(request, now_ticks).await;
            continue;
        }
        runtime.drain_requests(None, now_ticks());
        let (demand, periodic) = {
            let selected = BATTERY_DEMAND.resolve(now_ticks(), BATTERY_REQUESTS.close_generation());
            if let Some(event) = selected.event {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    selected.live,
                );
            }
            (selected.demand, selected.window)
        };
        let outcome = runtime.step(&demand, now_ticks).await;
        state_sender.send(runtime.state());
        crate::firmware::display::wake();
        // step marks terminal exhaustion and closes admission before control
        // can take a branch that would otherwise resume another acquisition.
        if runtime.is_stopped() {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), true) {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    BATTERY_DEMAND.live(),
                );
            }
            console::println!("BATTERY_STOP reason=RevisionExhausted");
            continue;
        }
        if let Some(request) = CONTROL.try_receive() {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), runtime.is_stopped()) {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    BATTERY_DEMAND.live(),
                );
            }
            runtime.apply_control(request, now_ticks).await;
            continue;
        }
        // No intervening await: these arrivals preceded conversion completion.
        runtime.drain_requests(Some(outcome), now_ticks());
        if matches!(outcome, StepOutcome::Acquired { .. }) {
            if let Some(sample) = periodic.and_then(|window| window.sample(runtime.state())) {
                console::println!("{}", sample);
            }
        }
        let deadline = match outcome {
            StepOutcome::Idle { next_wake } => next_wake,
            _ => Some(now_ticks()),
        };
        let deadline = observation::periodic::PeriodicWindow::deadline(periodic, deadline);
        let wake = async {
            match deadline {
                Some(at) => Timer::at(EmbassyInstant::from_millis(at.0)).await,
                None => core::future::pending::<()>().await,
            }
        };
        if let Either::First(request) = select(
            CONTROL.receive(),
            select(
                BATTERY_DEMAND.changed(),
                select(BATTERY_REQUESTS.ready_to_receive(), wake),
            ),
        )
        .await
        {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), runtime.is_stopped()) {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    BATTERY_PROVIDER_ID,
                    BATTERY_DEMAND.live(),
                );
            }
            runtime.apply_control(request, now_ticks).await;
        }
    }
}

pub(crate) fn control_snapshot() -> (
    crate::firmware::bounded_control::ControlRequest,
    Option<crate::firmware::bounded_control::ControlAck>,
) {
    CONTROL.snapshot()
}
