//! BME688 acquisition follows product authority demand. Admitted entry requests
//! share one bounded credit budget across ingress and the provider queue. A
//! compatible request arriving during conversion may use that completed sample.
//! Controls are checked before every step and after conversion, independently of
//! demand/request capacity. Conversion itself completes before quiescence.

use core::future::pending;
use embassy_futures::select::{select, Either};
use embassy_time::{Instant as EmbassyInstant, Timer};
use observation::ids::{ProviderGeneration, Revision};
use observation::runtime::{ProviderLoop, StepOutcome};
use observation::time::Instant as ObsInstant;

use crate::firmware::bounded_control::{ControlCommand, ControlRequest};
use crate::firmware::types::{InkplateEnvironmentDriver, SharedI2cBus};

use super::super::config::{
    ENVIRONMENT_DEMAND, ENVIRONMENT_REQUESTS, ENVIRONMENT_STATE, REQUEST_CAPACITY,
};
use super::super::driver::BmeDriver;
use super::super::types::{
    EnvironmentFields, EnvironmentSnapshot, EnvironmentStateSnapshot, ENVIRONMENT_PROVIDER_ID,
    FIELDS,
};
use super::acquisition_control::{handle_control_command, receive_command, try_receive_command};

fn now_ticks() -> ObsInstant {
    ObsInstant(EmbassyInstant::now().as_millis())
}

type EnvironmentProvider =
    ProviderLoop<EnvironmentFields, EnvironmentSnapshot, BmeDriver, REQUEST_CAPACITY, FIELDS>;

fn publish(provider: &EnvironmentProvider, last_sample_revision: Option<Revision>) {
    ENVIRONMENT_STATE
        .sender()
        .send(EnvironmentStateSnapshot::from_observation(
            provider.state(),
            last_sample_revision,
        ));
    crate::firmware::display::wake();
}

async fn control(provider: &mut EnvironmentProvider, command: ControlRequest) {
    if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), false) {
        crate::firmware::observation_fixture::report_periodic(
            event,
            ENVIRONMENT_PROVIDER_ID,
            ENVIRONMENT_DEMAND.live(),
        );
    }
    if matches!(command.command, ControlCommand::Suspend) {
        ENVIRONMENT_REQUESTS.close();
        let pending = provider.pending_observe_now();
        let outcome = provider.suspend(now_ticks()).await;
        ENVIRONMENT_REQUESTS.complete(pending);
        console::println!("BME688_SUSPEND outcome={:?}", outcome);
        let cleanly = outcome == observation::runtime::SuspendOutcome::Quiesced;
        handle_control_command(command, cleanly).await;
        provider.resume();
    } else {
        handle_control_command(command, true).await;
    }
    if !ENVIRONMENT_REQUESTS.open() {
        console::println!("BME688_REQUEST rejected=GenerationExhausted");
    }
}

#[embassy_executor::task]
pub async fn environment_acquisition_task(
    initial: Option<InkplateEnvironmentDriver>,
    bus: &'static SharedI2cBus,
) {
    let mut provider = EnvironmentProvider::new(
        BmeDriver::new(initial, bus),
        ENVIRONMENT_PROVIDER_ID,
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot {
            onboard: inkplate_tempera::environment::EnvironmentReading {
                temperature_centidegrees: 0,
                humidity_millipercent: 0,
            },
            external: None,
        },
    );
    let mut last_sample_revision = None;
    loop {
        if let Some(command) = try_receive_command() {
            control(&mut provider, command).await;
            continue;
        }
        drain_requests(&mut provider, None);
        // Always read current demand after control and admission. Owner removal
        // during an in-flight step takes effect before another acquisition.
        let (demand, periodic) = {
            let selected =
                ENVIRONMENT_DEMAND.resolve(now_ticks(), ENVIRONMENT_REQUESTS.close_generation());
            if let Some(event) = selected.event {
                crate::firmware::observation_fixture::report_periodic(
                    event,
                    ENVIRONMENT_PROVIDER_ID,
                    selected.live,
                );
            }
            (selected.demand, selected.window)
        };
        let before = provider.pending_observe_now();
        let outcome = provider.step(&demand, now_ticks).await;
        ENVIRONMENT_REQUESTS.complete(before - provider.pending_observe_now());
        if let StepOutcome::Acquired { revision, .. } = outcome {
            last_sample_revision = Some(revision);
        }
        publish(&provider, last_sample_revision);
        if let Some(command) = try_receive_command() {
            control(&mut provider, command).await;
            continue;
        }
        // No intervening await: every request drained here was admitted before
        // this step completed (including equal millisecond timestamps).
        drain_requests(&mut provider, Some(outcome));

        if matches!(outcome, StepOutcome::Acquired { .. }) {
            if let Some(sample) = periodic.and_then(|window| {
                window.sample(EnvironmentStateSnapshot::from_observation(
                    provider.state(),
                    last_sample_revision,
                ))
            }) {
                console::println!("{}", sample);
            }
        }
        let deadline = match outcome {
            StepOutcome::Idle { next_wake } => next_wake,
            StepOutcome::RevisionExhausted => {
                ENVIRONMENT_REQUESTS.close();
                if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), true) {
                    crate::firmware::observation_fixture::report_periodic(
                        event,
                        ENVIRONMENT_PROVIDER_ID,
                        ENVIRONMENT_DEMAND.live(),
                    );
                }
                console::println!("BME688_STOP reason=RevisionExhausted");
                crate::firmware::reset_pending_update_or_halt();
            }
            // Re-evaluate expiry, new demand and queued fields; the core enforces
            // start spacing and completion-based retry deadlines on every wake.
            _ => Some(now_ticks()),
        };
        let deadline = observation::periodic::PeriodicWindow::deadline(periodic, deadline);
        let wake = async {
            match deadline {
                Some(at) => Timer::at(EmbassyInstant::from_millis(at.0)).await,
                None => pending::<()>().await,
            }
        };
        if let Either::First(command) = select(
            receive_command(),
            select(
                ENVIRONMENT_DEMAND.changed(),
                select(ENVIRONMENT_REQUESTS.ready_to_receive(), wake),
            ),
        )
        .await
        {
            control(&mut provider, command).await;
        }
    }
}

fn drain_requests(
    provider: &mut EnvironmentProvider,
    outcome: Option<StepOutcome<EnvironmentFields>>,
) {
    // Credits guarantee that ingress plus provider pending never exceeds this
    // same capacity. The bound also prevents a producer from starving control.
    for _ in 0..REQUEST_CAPACITY {
        let Ok(request) = ENVIRONMENT_REQUESTS.try_receive() else {
            break;
        };
        let before = provider.pending_observe_now();
        let admitted = match outcome {
            Some(outcome) => provider.admit_observe_now_after_step(request, outcome, now_ticks()),
            None => provider.admit_observe_now_at(request, now_ticks()),
        };
        // Includes this dequeued credit when immediately satisfied/rejected,
        // and any expired older provider requests purged during admission.
        ENVIRONMENT_REQUESTS.complete(before + 1 - provider.pending_observe_now());
        if let Err(reason) = admitted {
            console::println!(
                "BME688_REQUEST rejected={:?} owner={} generation={}",
                reason,
                request.owner.0,
                request.owner_generation.0,
            );
        }
    }
}
