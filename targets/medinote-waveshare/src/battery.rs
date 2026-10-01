//! Medinote battery observation provider.
//! Contract: [ADR-0018](../../../docs/architecture/0018-typed-observation-subscriptions.md).
//!
//! Moved out of `runtime_ui`'s single task alongside the environmental
//! provider ([`crate::environment`]), for the same reason: acquisition no
//! longer blocks button sampling and physics/render. The original read used
//! `Adc::read_blocking`, a genuine busy-spin (`while !ADCX::is_done() {}`
//! inside esp-hal) -- wrapping that in an async timeout would still hog the
//! CPU for the timeout's duration, so this driver instead polls esp-hal's
//! non-blocking `read_oneshot` and yields between polls, bounded by a wall-
//! clock timeout, so other tasks actually run while a conversion is in
//! flight.
//!
//! Sleep closes request admission and awaits provider cleanup. Accepted one-shot
//! reads survive ordinary demand withdrawal, but never cross CPU sleep.

#[cfg(target_os = "none")]
use crate::environment::control;
#[cfg(target_os = "none")]
use embassy_futures::select::{select, Either};
use embassy_time::{Duration as EmbassyDuration, TimeoutError};
#[cfg(target_os = "none")]
use embassy_time::{Instant as EmbassyInstant, Timer};
#[cfg(target_os = "none")]
use esp_hal::analog::adc::{Adc, AdcCalCurve, AdcPin};
#[cfg(target_os = "none")]
use esp_hal::peripherals::{ADC1, GPIO4};
#[cfg(target_os = "none")]
use esp_hal::Blocking;

#[cfg(target_os = "none")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(target_os = "none")]
use embassy_sync::watch::Watch;

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

use medinote::presentation::battery_percent_from_mv;

pub use medinote::observations::{BatteryFields, BatterySnapshot};
#[cfg(target_os = "none")]
use medinote::observations::{BatteryStateSnapshot, BATTERY_PROVIDER_ID, FIELDS};

/// Shared capacity across ingress and the provider queue.
#[cfg(target_os = "none")]
const REQUEST_CAPACITY: usize = 2;
#[cfg(target_os = "none")]
const STATE_RECEIVERS: usize = 1;

#[cfg(target_os = "none")]
pub static BATTERY_DEMAND: DemandControl<CriticalSectionRawMutex, BatteryFields, FIELDS> =
    DemandControl::new();
#[cfg(target_os = "none")]
pub static BATTERY_STATE: Watch<CriticalSectionRawMutex, BatteryStateSnapshot, STATE_RECEIVERS> =
    Watch::new();

#[cfg(target_os = "none")]
pub(crate) static BATTERY_REQUESTS: RequestIngress<
    CriticalSectionRawMutex,
    BatteryFields,
    REQUEST_CAPACITY,
> = RequestIngress::new();

#[cfg(target_os = "none")]
pub(crate) static BATTERY_CONTROL: control::Control<CriticalSectionRawMutex> =
    control::Control::new();

#[cfg(target_os = "none")]
fn now_ticks() -> ObsInstant {
    ObsInstant(embassy_time::Instant::now().as_millis())
}

/// A single conversion this driver will wait at most this long for, polling
/// rather than spinning across it. esp-hal's SAR ADC conversions complete in
/// microseconds under normal operation; this bound exists to guarantee the
/// provider task cannot hang indefinitely on stuck hardware, not because a
/// real conversion is expected to approach it.
const CONVERSION_TIMEOUT: EmbassyDuration = EmbassyDuration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdcTimeout;

impl From<TimeoutError> for AdcTimeout {
    fn from(_: TimeoutError) -> Self {
        AdcTimeout
    }
}

/// Narrow conversion boundary: the target owns the peripheral and pin; host
/// tests exercise the same timeout/drop guard with a controllable converter.
pub trait BatteryAdc {
    fn read(&mut self) -> nb::Result<u16, ()>;
    /// Abandons any conversion in flight and reports whether the converter
    /// ended up quiesced. `false` means a conversion is still latched in
    /// hardware and the next [`BatteryAdc::read`] will observe it rather than
    /// starting a fresh one — the caller must not report clean cancellation.
    fn cancel(&mut self) -> bool;
}

#[cfg(target_os = "none")]
struct HalBatteryAdc {
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO4<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
    /// Mirrors esp-hal's private `Adc::active_channel` for our one channel: a
    /// conversion has been started and not yet consumed. `cancel` needs this
    /// because draining is a `read_oneshot` call, and on an idle converter
    /// that call *starts* a conversion instead of finishing one — which would
    /// leave the ADC busier than it found it.
    in_flight: bool,
}

#[cfg(target_os = "none")]
impl BatteryAdc for HalBatteryAdc {
    fn read(&mut self) -> nb::Result<u16, ()> {
        let result = self.adc.read_oneshot(&mut self.pin);
        // `read_oneshot` starts a conversion on its first call and clears its
        // own bookkeeping only when it hands back a value.
        self.in_flight = result.is_err();
        result
    }

    fn cancel(&mut self) -> bool {
        // esp-hal 1.2.0 removed the public `Adc::cancel_oneshot` this used to
        // call, and `read_oneshot`'s in-progress bookkeeping (`active_channel`)
        // is private with no public way to reset it.
        //
        // Left set, that state is not lost: the next `read_oneshot` for the
        // same channel -- and this driver only ever reads one -- sees
        // `active_channel` already matching, skips `start_sample`, and returns
        // whatever the hardware latched while we were away. One stale reading,
        // then normal fresh conversions. So nothing hangs either way.
        //
        // But "recovers on the next read" is not the same as quiesced, and
        // that difference matters here: `AcquisitionDriver::cancel` is called
        // from provider suspend, where the next read can be on the far side of
        // a sleep. So drain it now instead of leaving it for whoever reads
        // next -- a single poll consumes the abandoned conversion, which is
        // what clears `active_channel` and runs the ADC's own `reset()`.
        //
        // One poll, never a loop: if the conversion genuinely has not finished
        // we are in the stuck-hardware case `CONVERSION_TIMEOUT` exists for,
        // and blocking here would reintroduce the hang that bound prevents.
        if !self.in_flight {
            return true;
        }
        let drained = self.adc.read_oneshot(&mut self.pin).is_ok();
        self.in_flight = !drained;
        drained
    }
}

struct ConversionGuard<'a, ADC: BatteryAdc> {
    adc: &'a mut ADC,
    complete: bool,
}

impl<ADC: BatteryAdc> Drop for ConversionGuard<'_, ADC> {
    fn drop(&mut self) {
        if !self.complete {
            // Drop cannot report, and the next read recovers on its own even
            // when the drain does not land; `AcquisitionDriver::cancel` is the
            // path where the outcome is actionable.
            let _ = self.adc.cancel();
        }
    }
}

pub struct AdcBatteryDriver<ADC: BatteryAdc> {
    adc: ADC,
}

impl<ADC: BatteryAdc> AdcBatteryDriver<ADC> {
    pub const fn new(adc: ADC) -> Self {
        Self { adc }
    }

    /// Polls esp-hal's non-blocking `read_oneshot`, yielding to the executor
    /// between polls instead of spinning, bounded by [`CONVERSION_TIMEOUT`].
    async fn read_yielding(&mut self) -> Result<u16, AdcTimeout> {
        let mut conversion = ConversionGuard {
            adc: &mut self.adc,
            complete: false,
        };
        embassy_time::with_timeout(CONVERSION_TIMEOUT, async {
            loop {
                match conversion.adc.read() {
                    Ok(value) => {
                        conversion.complete = true;
                        return value;
                    }
                    Err(nb::Error::WouldBlock) => embassy_futures::yield_now().await,
                    // esp-hal's `Err` type here is `()`: this arm exists only
                    // for exhaustiveness and is unreachable in practice.
                    Err(nb::Error::Other(())) => embassy_futures::yield_now().await,
                }
            }
        })
        .await
        .map_err(AdcTimeout::from)
    }
}

impl<ADC: BatteryAdc> AcquisitionDriver<BatteryFields, BatterySnapshot> for AdcBatteryDriver<ADC> {
    type Error = AdcTimeout;

    async fn acquire(
        &mut self,
        _requested: BatteryFields,
    ) -> Result<(BatteryFields, BatterySnapshot), Self::Error> {
        let reading = self.read_yielding().await;
        #[cfg(target_os = "none")]
        if let Err(error) = &reading {
            console::println!("ADC_ACQUIRE_FAILED error={:?}", error);
        }
        let pin_mv = reading? as u32;
        let millivolts = pin_mv * 3; // the board's fixed divider ratio, unchanged from `runtime_ui`'s original.
        let snapshot = BatterySnapshot {
            millivolts,
            percent: battery_percent_from_mv(millivolts),
        };
        Ok((BatteryFields::VOLTAGE.union(BatteryFields::LEVEL), snapshot))
    }

    async fn cancel(&mut self) -> Result<(), Self::Error> {
        // `ProviderLoop::suspend` turns this into `Quiesced` / `CleanupFailed`,
        // so it must not claim a cleanup the driver did not achieve. A
        // converter still holding a latched conversion is the stuck-hardware
        // case, which is what `AdcTimeout` already means on this driver.
        if self.adc.cancel() {
            Ok(())
        } else {
            Err(AdcTimeout)
        }
    }

    fn min_acquisition_interval(&self) -> ObsDuration {
        ObsDuration::ZERO // the active demand's `max_age` is the only cadence floor this needs.
    }
}

#[cfg(target_os = "none")]
#[embassy_executor::task]
pub async fn battery_provider_task(
    adc: Adc<'static, ADC1<'static>, Blocking>,
    pin: AdcPin<GPIO4<'static>, ADC1<'static>, AdcCalCurve<ADC1<'static>>>,
) {
    let driver = AdcBatteryDriver::new(HalBatteryAdc {
        adc,
        pin,
        in_flight: false,
    });
    let mut provider =
        ProviderLoop::<BatteryFields, BatterySnapshot, _, REQUEST_CAPACITY, FIELDS>::new(
            driver,
            BATTERY_PROVIDER_ID,
            ProviderGeneration::INITIAL,
            BatterySnapshot::default(),
        );
    let state_sender = BATTERY_STATE.sender();
    let mut last_sample_revision = None;
    loop {
        // Control is independent of request capacity and wins before each step.
        if let Some(request) = BATTERY_CONTROL.try_receive() {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(BATTERY_PROVIDER_ID, BATTERY_DEMAND.live())
                );
            }
            BATTERY_CONTROL
                .handle(
                    &mut provider,
                    &BATTERY_REQUESTS,
                    "BATTERY",
                    request,
                    now_ticks,
                )
                .await;
            continue;
        }
        control::drain_requests(&mut provider, &BATTERY_REQUESTS, None, now_ticks, "BATTERY");
        // Read the current Watch value even after conversion: a withdrawal must
        // take effect before another periodic acquisition can start.
        let (demand, periodic) = {
            let selected = BATTERY_DEMAND.resolve(now_ticks(), BATTERY_REQUESTS.close_generation());
            if let Some(event) = selected.event {
                console::println!("{}", event.report(BATTERY_PROVIDER_ID, selected.live));
            }
            (selected.demand, selected.window)
        };
        let before = provider.pending_observe_now();
        let outcome = provider.step(&demand, now_ticks).await;
        if let StepOutcome::Acquired { revision, .. } = outcome {
            last_sample_revision = Some(revision);
        }
        BATTERY_REQUESTS.complete(before - provider.pending_observe_now());
        let state = provider.state();
        state_sender.send(BatteryStateSnapshot::from_observation(
            state,
            last_sample_revision,
        ));

        if matches!(outcome, StepOutcome::RevisionExhausted) {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), true) {
                console::println!(
                    "{}",
                    event.report(BATTERY_PROVIDER_ID, BATTERY_DEMAND.live())
                );
            }
            BATTERY_REQUESTS.close();
            console::println!("OBSERVATION_STOP provider=BATTERY reason=RevisionExhausted");
            BATTERY_CONTROL
                .serve_stopped(&mut provider, &BATTERY_REQUESTS, "BATTERY", now_ticks)
                .await
        }
        if let Some(request) = BATTERY_CONTROL.try_receive() {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(BATTERY_PROVIDER_ID, BATTERY_DEMAND.live())
                );
            }
            BATTERY_CONTROL
                .handle(
                    &mut provider,
                    &BATTERY_REQUESTS,
                    "BATTERY",
                    request,
                    now_ticks,
                )
                .await;
            continue;
        }
        // No intervening await: queued arrivals belong to this completed step.
        control::drain_requests(
            &mut provider,
            &BATTERY_REQUESTS,
            Some(outcome),
            now_ticks,
            "BATTERY",
        );
        if matches!(outcome, StepOutcome::Acquired { .. }) {
            if let Some(sample) = periodic.and_then(|window| {
                window.sample(BatteryStateSnapshot::from_observation(
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
            BATTERY_CONTROL.receive(),
            select(
                BATTERY_DEMAND.changed(),
                select(BATTERY_REQUESTS.ready_to_receive(), wake),
            ),
        )
        .await
        {
            if let Some(event) = BATTERY_DEMAND.end(now_ticks(), false) {
                console::println!(
                    "{}",
                    event.report(BATTERY_PROVIDER_ID, BATTERY_DEMAND.live())
                );
            }
            BATTERY_CONTROL
                .handle(
                    &mut provider,
                    &BATTERY_REQUESTS,
                    "BATTERY",
                    request,
                    now_ticks,
                )
                .await;
        }
    }
}
