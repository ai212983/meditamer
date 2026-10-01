//! Medinote environmental (SHTC3 temperature/humidity) observation provider.
//! Contract: [ADR-0018](../../../docs/architecture/0018-typed-observation-subscriptions.md).
//!
//! Moved out of `runtime_ui`'s single task so a ~70ms wakeup/settle/
//! conversion sequence no longer blocks button sampling and (on the
//! Hourglass screen) physics/render for that long. This task owns the
//! sensor, its timing, and its cleanup; `runtime_ui` only sees the shared
//! [`observation`] contract: it publishes demand, reads published state, and
//! decides delivery -- it never touches the I2C bus.
//!
//! `shtc3` is reached directly here, not through `boards/waveshare-rlcd42`,
//! matching this target's existing choice recorded in its `Cargo.toml`
//! ("the board's only consumer for this sensor") from before this plan --
//! board hardware ownership for *this* sensor already stopped at the driver
//! crate, not a board module.
//!
//! Home owns periodic demand and bounded fresh-entry requests. Navigation
//! withdrawal keeps accepted one-shots; suspension closes and clears both queues.

#[cfg(target_os = "none")]
use embassy_futures::select::{select, Either};
#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::watch::Watch;
#[cfg(target_os = "none")]
use embassy_time::Instant as EmbassyInstant;
use embassy_time::{Duration as EmbassyDuration, Timer};

use observation::field::FieldMask;
#[cfg(target_os = "none")]
use observation::ids::ProviderGeneration;
#[cfg(target_os = "none")]
use observation::ingress::RequestIngress;
#[cfg(target_os = "none")]
use observation::periodic::DemandControl;
use observation::runtime::AcquisitionDriver;
#[cfg(target_os = "none")]
use observation::runtime::{ProviderLoop, StepOutcome};
use observation::time::Duration as ObsDuration;
#[cfg(target_os = "none")]
use observation::time::Instant as ObsInstant;

#[cfg(target_os = "none")]
use crate::SharedI2c;

pub use medinote::observations::{EnvironmentFields, EnvironmentSnapshot};
#[cfg(target_os = "none")]
use medinote::observations::{EnvironmentStateSnapshot, ENVIRONMENT_PROVIDER_ID, FIELDS};

/// Shared capacity across ingress and the provider queue.
#[cfg(target_os = "none")]
const REQUEST_CAPACITY: usize = 2;
#[cfg(target_os = "none")]
const STATE_RECEIVERS: usize = 1;

#[cfg(target_os = "none")]
pub static ENVIRONMENT_DEMAND: DemandControl<CriticalSectionRawMutex, EnvironmentFields, FIELDS> =
    DemandControl::new();
#[cfg(target_os = "none")]
pub static ENVIRONMENT_STATE: Watch<
    CriticalSectionRawMutex,
    EnvironmentStateSnapshot,
    STATE_RECEIVERS,
> = Watch::new();
#[path = "environment/control.rs"]
pub(crate) mod control;

/// Sleep admission is correlated with the provider's exact cleanup request.
/// Only the UI task publishes intent; only this provider acknowledges it.
#[cfg(target_os = "none")]
pub(crate) static ENVIRONMENT_CONTROL: control::Control<CriticalSectionRawMutex> =
    control::Control::new();

#[cfg(target_os = "none")]
pub(crate) static ENVIRONMENT_REQUESTS: RequestIngress<
    CriticalSectionRawMutex,
    EnvironmentFields,
    REQUEST_CAPACITY,
> = RequestIngress::new();

#[cfg(target_os = "none")]
fn now_ticks() -> ObsInstant {
    ObsInstant(EmbassyInstant::now().as_millis())
}

// Whole-operation limits include shared-bus admission and every transaction.
// Normal measurement contains 70ms of settling; cleanup has its own budget
// after a timed-out measurement so sleep is still attempted.
const ACQUISITION_TIMEOUT: EmbassyDuration = EmbassyDuration::from_millis(250);
const CLEANUP_TIMEOUT: EmbassyDuration = EmbassyDuration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shtc3Failure<E> {
    Sensor(shtc3::Error<E>),
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shtc3Error<E> {
    Acquisition(Shtc3Failure<E>),
    Cleanup {
        acquisition: Option<Shtc3Failure<E>>,
        cleanup: Shtc3Failure<E>,
    },
}

/// Wraps `shtc3::Shtc3` with the wakeup/settle/conversion/read/sleep timing
/// Waveshare's example uses. Board/enclosure temperature and humidity
/// calibration are passed in by the caller so this remains a reusable sensor
/// wrapper rather than a product-policy holder.
pub struct Shtc3Driver<I2C> {
    sensor: shtc3::Shtc3<I2C>,
    self_heating_correction_millicelsius: i32,
    humidity_scale_permille: i32,
}

impl<I2C> Shtc3Driver<I2C> {
    pub const fn new(
        sensor: shtc3::Shtc3<I2C>,
        self_heating_correction_millicelsius: i32,
        humidity_scale_permille: i32,
    ) -> Self {
        Self {
            sensor,
            self_heating_correction_millicelsius,
            humidity_scale_permille,
        }
    }
}

const fn scale_humidity_millipercent(humidity_millipercent: i32, scale_permille: i32) -> i32 {
    let scaled = humidity_millipercent
        .saturating_mul(scale_permille)
        .saturating_add(500)
        / 1_000;
    if scaled < 0 {
        0
    } else if scaled > 100_000 {
        100_000
    } else {
        scaled
    }
}

impl<I2C: embedded_hal_async::i2c::I2c> Shtc3Driver<I2C> {
    async fn cleanup(&mut self) -> Result<(), Shtc3Failure<I2C::Error>> {
        embassy_time::with_timeout(CLEANUP_TIMEOUT, self.sensor.sleep())
            .await
            .map_err(|_| Shtc3Failure::TimedOut)?
            .map_err(Shtc3Failure::Sensor)
    }

    async fn measure(
        &mut self,
    ) -> Result<(EnvironmentFields, EnvironmentSnapshot), shtc3::Error<I2C::Error>> {
        self.sensor.wakeup().await?;
        Timer::after(EmbassyDuration::from_millis(50)).await;
        self.sensor.start_measurement().await?;
        Timer::after(EmbassyDuration::from_millis(20)).await;
        let measurement = self.sensor.read_measurement().await?;
        let snapshot = EnvironmentSnapshot {
            temperature_millicelsius: measurement.temperature_millicelsius
                - self.self_heating_correction_millicelsius,
            humidity_millipercent: scale_humidity_millipercent(
                measurement.humidity_millipercent,
                self.humidity_scale_permille,
            ),
        };
        Ok((
            EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
            snapshot,
        ))
    }
}

impl<I2C: embedded_hal_async::i2c::I2c> AcquisitionDriver<EnvironmentFields, EnvironmentSnapshot>
    for Shtc3Driver<I2C>
{
    type Error = Shtc3Error<I2C::Error>;

    async fn acquire(
        &mut self,
        _requested: EnvironmentFields,
    ) -> Result<(EnvironmentFields, EnvironmentSnapshot), Self::Error> {
        let measurement =
            match embassy_time::with_timeout(ACQUISITION_TIMEOUT, self.measure()).await {
                Ok(outcome) => outcome.map_err(Shtc3Failure::Sensor),
                Err(_) => Err(Shtc3Failure::TimedOut),
            };
        // Dropping the timed measurement releases its bus borrow before
        // cleanup starts. A sample is committed only after sleep succeeds;
        // either failure leaves ProviderLoop's previous sample intact.
        let outcome = match (measurement, self.cleanup().await) {
            (Ok(sample), Ok(())) => Ok(sample),
            (Err(acquisition), Ok(())) => Err(Shtc3Error::Acquisition(acquisition)),
            (measurement, Err(cleanup)) => Err(Shtc3Error::Cleanup {
                acquisition: measurement.err(),
                cleanup,
            }),
        };
        #[cfg(target_os = "none")]
        if let Err(error) = &outcome {
            console::println!("SHTC3_ACQUIRE_FAILED error={:?}", error);
        }
        outcome
    }

    async fn cancel(&mut self) -> Result<(), Self::Error> {
        let outcome = self.cleanup().await.map_err(|cleanup| Shtc3Error::Cleanup {
            acquisition: None,
            cleanup,
        });
        #[cfg(target_os = "none")]
        if let Err(error) = &outcome {
            console::println!("SHTC3_SUSPEND_FAILED error={:?}", error);
        }
        outcome
    }

    fn min_acquisition_interval(&self) -> ObsDuration {
        // The combined `max_age` carried by active demand is the only
        // cadence floor this sensor needs; it imposes no additional one.
        ObsDuration::ZERO
    }
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
pub async fn environment_provider_task(
    i2c: SharedI2c,
    self_heating_correction_millicelsius: i32,
    humidity_scale_permille: i32,
) {
    let driver = Shtc3Driver::new(
        shtc3::Shtc3::new(i2c),
        self_heating_correction_millicelsius,
        humidity_scale_permille,
    );
    let mut provider =
        ProviderLoop::<EnvironmentFields, EnvironmentSnapshot, _, REQUEST_CAPACITY, FIELDS>::new(
            driver,
            ENVIRONMENT_PROVIDER_ID,
            ProviderGeneration::INITIAL,
            EnvironmentSnapshot::default(),
        );
    let state_sender = ENVIRONMENT_STATE.sender();
    let mut last_sample_revision = None;
    loop {
        // Control is independent of request capacity and wins before each step.
        if let Some(request) = ENVIRONMENT_CONTROL.try_receive() {
            if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(ENVIRONMENT_PROVIDER_ID, ENVIRONMENT_DEMAND.live())
                );
            }
            ENVIRONMENT_CONTROL
                .handle(
                    &mut provider,
                    &ENVIRONMENT_REQUESTS,
                    "ENVIRONMENT",
                    request,
                    now_ticks,
                )
                .await;
            continue;
        }
        control::drain_requests(
            &mut provider,
            &ENVIRONMENT_REQUESTS,
            None,
            now_ticks,
            "ENVIRONMENT",
        );
        // Read current effective demand even after conversion: a withdrawal must
        // take effect before another periodic acquisition can start.
        let (demand, periodic) = {
            let selected =
                ENVIRONMENT_DEMAND.resolve(now_ticks(), ENVIRONMENT_REQUESTS.close_generation());
            if let Some(event) = selected.event {
                console::println!("{}", event.report(ENVIRONMENT_PROVIDER_ID, selected.live));
            }
            (selected.demand, selected.window)
        };
        let before = provider.pending_observe_now();
        let outcome = provider.step(&demand, now_ticks).await;
        if let StepOutcome::Acquired { revision, .. } = outcome {
            last_sample_revision = Some(revision);
        }
        ENVIRONMENT_REQUESTS.complete(before - provider.pending_observe_now());
        let state = provider.state();
        state_sender.send(EnvironmentStateSnapshot::from_observation(
            state,
            last_sample_revision,
        ));

        if matches!(outcome, StepOutcome::RevisionExhausted) {
            if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), true) {
                console::println!(
                    "{}",
                    event.report(ENVIRONMENT_PROVIDER_ID, ENVIRONMENT_DEMAND.live())
                );
            }
            ENVIRONMENT_REQUESTS.close();
            console::println!("OBSERVATION_STOP provider=ENVIRONMENT reason=RevisionExhausted");
            ENVIRONMENT_CONTROL
                .serve_stopped(
                    &mut provider,
                    &ENVIRONMENT_REQUESTS,
                    "ENVIRONMENT",
                    now_ticks,
                )
                .await
        }
        if let Some(request) = ENVIRONMENT_CONTROL.try_receive() {
            if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(ENVIRONMENT_PROVIDER_ID, ENVIRONMENT_DEMAND.live())
                );
            }
            ENVIRONMENT_CONTROL
                .handle(
                    &mut provider,
                    &ENVIRONMENT_REQUESTS,
                    "ENVIRONMENT",
                    request,
                    now_ticks,
                )
                .await;
            continue;
        }
        // No intervening await: queued arrivals belong to this completed step.
        control::drain_requests(
            &mut provider,
            &ENVIRONMENT_REQUESTS,
            Some(outcome),
            now_ticks,
            "ENVIRONMENT",
        );
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
            ENVIRONMENT_CONTROL.receive(),
            select(
                ENVIRONMENT_DEMAND.changed(),
                select(ENVIRONMENT_REQUESTS.ready_to_receive(), wake),
            ),
        )
        .await
        {
            if let Some(event) = ENVIRONMENT_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(ENVIRONMENT_PROVIDER_ID, ENVIRONMENT_DEMAND.live())
                );
            }
            ENVIRONMENT_CONTROL
                .handle(
                    &mut provider,
                    &ENVIRONMENT_REQUESTS,
                    "ENVIRONMENT",
                    request,
                    now_ticks,
                )
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::scale_humidity_millipercent;

    #[test]
    fn humidity_calibration_rounds_and_clamps_sensor_output() {
        assert_eq!(scale_humidity_millipercent(38_750, 1_169), 45_299);
        assert_eq!(scale_humidity_millipercent(-1, 1_169), 0);
        assert_eq!(scale_humidity_millipercent(100_000, 1_169), 100_000);
    }
}
