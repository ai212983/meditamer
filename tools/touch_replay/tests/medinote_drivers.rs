//! Exercise the production providers with real Embassy timers on a virtual
//! clock. Only peripheral transactions/conversions are substituted.

#[path = "../../../targets/medinote-waveshare/src/battery.rs"]
mod battery;
#[path = "../../../targets/medinote-waveshare/src/environment.rs"]
mod environment;

use core::future::{pending, poll_fn, ready, Future};
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};

use battery::{AdcBatteryDriver, AdcTimeout, BatteryAdc, BatteryFields, BatterySnapshot};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_hal_async::i2c::{ErrorKind, ErrorType, I2c, Operation};
use environment::control::{Command, Control, SuspendAck};
use environment::{EnvironmentFields, EnvironmentSnapshot, Shtc3Driver, Shtc3Error, Shtc3Failure};
use observation::demand::{aggregate_demand, Demand};
use observation::field::FieldMask;
use observation::ids::{
    OwnerGeneration, OwnerId, ProviderGeneration, ProviderId, SubscriptionSlot,
};
use observation::ingress::{RequestAdmissionError, RequestIngress};
use observation::observe_now::ObserveNowRequest;
use observation::policy::{Health, InitialPolicy};
use observation::runtime::{AcquisitionDriver, ProviderLoop, StepOutcome, SuspendOutcome};
use observation::subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey};
use observation::time::{Duration, Instant};

static CLOCK: Mutex<()> = Mutex::new(());

fn clock() -> MutexGuard<'static, ()> {
    let guard = CLOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    embassy_time::MockDriver::get().reset();
    guard
}

fn run<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..1000 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return result;
        }
        embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(1));
    }
    panic!("provider operation exceeded the test's 1s bound");
}

fn now() -> Instant {
    Instant(embassy_time::Instant::now().as_millis())
}

fn demand<F: FieldMask>() -> Demand<F, 2> {
    let mut book = SubscriptionBook::<F, 1>::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: F::from_bits_truncate(3),
        max_age: Duration::ZERO,
        min_delivery_interval: Duration::ZERO,
        initial_policy: InitialPolicy::UseCache,
        expires_at: None,
    })
    .unwrap();
    aggregate_demand(&book, ProviderId(1), now())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BusError;

impl embedded_hal_async::i2c::Error for BusError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Fault {
    #[default]
    None,
    Error,
    Hang,
}

#[derive(Clone, Default)]
struct Bus {
    acquisition: Rc<Cell<Fault>>,
    cleanup: Rc<Cell<Fault>>,
    trace: Rc<RefCell<Vec<&'static str>>>,
}

impl ErrorType for Bus {
    type Error = BusError;
}

struct PendingTransaction(Rc<RefCell<Vec<&'static str>>>);

impl Drop for PendingTransaction {
    fn drop(&mut self) {
        self.0.borrow_mut().push("cancel-transaction");
    }
}

impl I2c for Bus {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), BusError> {
        assert_eq!(address, shtc3::I2C_ADDRESS);
        for operation in operations {
            let (name, fault) = match operation {
                Operation::Write([0x35, 0x17]) => ("wake", Fault::None),
                Operation::Write([0x78, 0x66]) => ("measure", Fault::None),
                Operation::Write([0xb0, 0x98]) => ("sleep", self.cleanup.get()),
                Operation::Read(bytes) => {
                    let temperature = [0x62, 0xe4];
                    let humidity = [0x73, 0x33];
                    bytes.copy_from_slice(&[
                        temperature[0],
                        temperature[1],
                        shtc3::crc8(&temperature),
                        humidity[0],
                        humidity[1],
                        shtc3::crc8(&humidity),
                    ]);
                    ("read", self.acquisition.get())
                }
                _ => panic!("unexpected sensor transaction"),
            };
            self.trace.borrow_mut().push(name);
            match fault {
                Fault::None => {}
                Fault::Error => return Err(BusError),
                Fault::Hang => {
                    let _transaction = PendingTransaction(self.trace.clone());
                    pending::<()>().await;
                }
            }
        }
        Ok(())
    }
}

fn sensor(bus: &Bus) -> Shtc3Driver<Bus> {
    Shtc3Driver::new(shtc3::Shtc3::new(bus.clone()), 4_000, 1_000)
}

#[test]
fn cleanup_failure_rejects_successful_sample_and_preserves_previous_state() {
    let _clock = clock();
    let bus = Bus::default();
    let mut provider = ProviderLoop::<_, _, _, 1, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    assert!(matches!(
        run(provider.step(&demand(), now)),
        StepOutcome::Acquired { .. }
    ));
    let previous = provider.state().snapshot;
    let stamped = provider.state().field_timestamps.get(0);
    assert_eq!(previous.temperature_millicelsius, 18_601);
    assert_eq!(previous.humidity_millipercent, 44_999);
    bus.cleanup.set(Fault::Error);
    let outcome = run(provider.step(&demand(), now));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = outcome else {
        panic!("expected acquisition failure, got {outcome:?}");
    };
    assert_eq!(provider.state().snapshot, previous);
    assert_eq!(provider.state().field_timestamps.get(0), stamped);
    assert_eq!(provider.state().health, Health::Degraded);
    bus.cleanup.set(Fault::None);
    let transactions = bus.trace.borrow().len();
    assert_eq!(
        run(provider.step(&demand(), now)),
        StepOutcome::Idle {
            next_wake: Some(retry_at)
        }
    );
    assert_eq!(bus.trace.borrow().len(), transactions);
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(
        retry_at.0.checked_sub(now().0).unwrap(),
    ));
    assert!(matches!(
        run(provider.step(&demand(), now)),
        StepOutcome::Acquired { .. }
    ));
}

#[test]
fn cleanup_failure_preserves_both_error_causes() {
    let _clock = clock();
    let bus = Bus::default();
    bus.acquisition.set(Fault::Error);
    bus.cleanup.set(Fault::Error);
    assert_eq!(
        run(sensor(&bus).acquire(EnvironmentFields::TEMPERATURE)),
        Err(Shtc3Error::Cleanup {
            acquisition: Some(Shtc3Failure::Sensor(shtc3::Error::Bus(BusError))),
            cleanup: Shtc3Failure::Sensor(shtc3::Error::Bus(BusError)),
        })
    );
    assert_eq!(&*bus.trace.borrow(), &["wake", "measure", "read", "sleep"]);
}

#[test]
fn acquisition_timeout_cancels_transaction_before_attempting_cleanup() {
    let _clock = clock();
    let bus = Bus::default();
    bus.acquisition.set(Fault::Hang);
    assert_eq!(
        run(sensor(&bus).acquire(EnvironmentFields::TEMPERATURE)),
        Err(Shtc3Error::Acquisition(Shtc3Failure::TimedOut))
    );
    assert_eq!(now(), Instant(250));
    assert_eq!(
        &*bus.trace.borrow(),
        &["wake", "measure", "read", "cancel-transaction", "sleep"]
    );
}

#[test]
fn timeout_and_cleanup_timeout_are_both_reported_with_separate_budgets() {
    let _clock = clock();
    let bus = Bus::default();
    bus.acquisition.set(Fault::Hang);
    bus.cleanup.set(Fault::Hang);
    assert_eq!(
        run(sensor(&bus).acquire(EnvironmentFields::HUMIDITY)),
        Err(Shtc3Error::Cleanup {
            acquisition: Some(Shtc3Failure::TimedOut),
            cleanup: Shtc3Failure::TimedOut,
        })
    );
    assert_eq!(now(), Instant(350));
    assert_eq!(bus.trace.borrow().last(), Some(&"cancel-transaction"));
}

#[test]
fn successful_measurement_with_cleanup_timeout_is_not_delivered() {
    let _clock = clock();
    let bus = Bus::default();
    bus.cleanup.set(Fault::Hang);
    assert_eq!(
        run(sensor(&bus).acquire(EnvironmentFields::TEMPERATURE)),
        Err(Shtc3Error::Cleanup {
            acquisition: None,
            cleanup: Shtc3Failure::TimedOut
        })
    );
    assert_eq!(now(), Instant(170));
}

#[test]
fn suspension_reports_cleanup_error_or_timeout_and_can_recover() {
    let _clock = clock();
    let bus = Bus::default();
    let mut provider = ProviderLoop::<_, _, _, 1, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    for fault in [Fault::Error, Fault::Hang] {
        bus.cleanup.set(fault);
        assert_eq!(run(provider.suspend(now())), SuspendOutcome::CleanupFailed);
        assert_eq!(run(provider.step(&demand(), now)), StepOutcome::Suspended);
        provider.resume();
    }
    bus.cleanup.set(Fault::None);
    assert_eq!(run(provider.suspend(now())), SuspendOutcome::Quiesced);
    assert_eq!(
        bus.trace
            .borrow()
            .iter()
            .filter(|&&event| event == "sleep")
            .count(),
        3
    );
}

/// `ready` and `stuck` model two different things, and the difference is what
/// `cancel` turns on. `ready` is whether a conversion completes within the
/// poll that observes it: false means "still converting", which is the normal
/// state during a `read_yielding` wait. `stuck` is whether the converter ever
/// completes at all. Real hardware converts in microseconds, so a conversion
/// abandoned at the 50ms `CONVERSION_TIMEOUT` has always finished by the time
/// `cancel` runs and can be drained — `stuck` is how a test asks for the
/// pathological case that bound actually exists to bound.
#[derive(Clone, Default)]
struct Adc {
    active: Rc<Cell<bool>>,
    ready: Rc<Cell<bool>>,
    stuck: Rc<Cell<bool>>,
    starts: Rc<Cell<u32>>,
    cancels: Rc<Cell<u32>>,
}

impl BatteryAdc for Adc {
    fn read(&mut self) -> nb::Result<u16, ()> {
        if !self.active.replace(true) {
            self.starts.set(self.starts.get() + 1);
        }
        if self.ready.get() {
            self.active.set(false);
            Ok(1_200)
        } else {
            Err(nb::Error::WouldBlock)
        }
    }

    /// Mirrors `HalBatteryAdc::cancel`. An idle converter is already quiesced
    /// and must not be touched -- draining is a `read`, which would otherwise
    /// start a conversion rather than finish one. An in-flight conversion is
    /// drained unless the converter is [`Adc::stuck`], i.e. never completes.
    fn cancel(&mut self) -> bool {
        self.cancels.set(self.cancels.get() + 1);
        if !self.active.get() {
            return true;
        }
        if self.stuck.get() {
            return false;
        }
        self.active.set(false);
        true
    }
}

#[test]
fn adc_timeout_resets_conversion_before_next_acquisition() {
    let _clock = clock();
    let adc = Adc::default();
    let mut driver = AdcBatteryDriver::new(adc.clone());
    assert_eq!(run(driver.acquire(BatteryFields::LEVEL)), Err(AdcTimeout));
    assert_eq!(now(), Instant(50));
    assert!(!adc.active.get());
    assert_eq!(adc.cancels.get(), 1);
    adc.ready.set(true);
    let (_, sample) = run(driver.acquire(BatteryFields::LEVEL)).unwrap();
    assert_eq!(sample.millivolts, 3_600);
    assert_eq!(sample.percent, 53);
    assert_eq!(adc.starts.get(), 2);
    assert_eq!(adc.cancels.get(), 1, "completed reads need no extra reset");
}

#[test]
fn dropping_adc_acquisition_resets_conversion_without_async_cleanup() {
    let _clock = clock();
    let adc = Adc::default();
    let mut driver = AdcBatteryDriver::new(adc.clone());
    {
        let mut read = core::pin::pin!(driver.acquire(BatteryFields::VOLTAGE));
        assert!(read
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        assert!(adc.active.get());
    }
    assert!(!adc.active.get());
    assert_eq!(adc.cancels.get(), 1);
    adc.ready.set(true);
    assert!(run(driver.acquire(BatteryFields::LEVEL)).is_ok());
    assert_eq!(adc.starts.get(), 2);
    assert_eq!(run(driver.cancel()), Ok(()));
    assert_eq!(adc.cancels.get(), 2);
}

/// esp-hal 1.2.0 removed `Adc::cancel_oneshot`, so `HalBatteryAdc::cancel`
/// quiesces by draining the abandoned conversion instead. A drain can fail,
/// and `ProviderLoop::suspend` maps this driver's cancel straight onto
/// `Quiesced` / `CleanupFailed` -- so a failed drain has to be reported rather
/// than swallowed into an unconditional `Ok`.
#[test]
fn stuck_converter_reports_failed_cleanup_instead_of_a_false_quiesce() {
    let _clock = clock();
    let adc = Adc::default();
    adc.stuck.set(true);
    let mut driver = AdcBatteryDriver::new(adc.clone());

    assert_eq!(run(driver.acquire(BatteryFields::LEVEL)), Err(AdcTimeout));
    assert!(
        adc.active.get(),
        "a conversion the converter never completes stays in flight"
    );
    assert_eq!(run(driver.cancel()), Err(AdcTimeout));
}

/// The other half of the same contract: draining is a read, so on an idle
/// converter it would *start* a conversion rather than finish one, leaving the
/// ADC busier than it found it. An idle converter is already quiesced.
#[test]
fn cancelling_an_idle_converter_reports_quiesced_without_starting_a_conversion() {
    let _clock = clock();
    let adc = Adc::default();
    let mut driver = AdcBatteryDriver::new(adc.clone());

    assert_eq!(run(driver.cancel()), Ok(()));
    assert_eq!(adc.starts.get(), 0);
    assert!(!adc.active.get());
}

#[test]
fn adc_timeout_retains_last_successful_sample_and_recovers() {
    let _clock = clock();
    let adc = Adc::default();
    adc.ready.set(true);
    let mut provider = ProviderLoop::<_, _, _, 1, 2>::new(
        AdcBatteryDriver::new(adc.clone()),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        BatterySnapshot::default(),
    );
    assert!(matches!(
        run(provider.step(&demand(), now)),
        StepOutcome::Acquired { .. }
    ));
    let snapshot = provider.state().snapshot;
    let stamp = provider.state().field_timestamps.get(0);
    adc.ready.set(false);
    let outcome = run(provider.step(&demand(), now));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = outcome else {
        panic!("expected acquisition failure, got {outcome:?}");
    };
    assert_eq!(provider.state().snapshot, snapshot);
    assert_eq!(provider.state().field_timestamps.get(0), stamp);
    adc.ready.set(true);
    assert_eq!(
        run(provider.step(&demand(), now)),
        StepOutcome::Idle {
            next_wake: Some(retry_at)
        }
    );
    assert_eq!(adc.starts.get(), 2);
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(
        retry_at.0.checked_sub(now().0).unwrap(),
    ));
    assert!(matches!(
        run(provider.step(&demand(), now)),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(adc.starts.get(), 3);
}

// Sleep-control tests drive the production actor with deliberately delayed
// cleanup. No timer or peripheral is needed to choose the interleaving.
type SleepControl = Control<NoopRawMutex>;
type ControlledProvider = ProviderLoop<EnvironmentFields, EnvironmentSnapshot, CleanupDriver, 1, 2>;

#[derive(Clone, Default)]
struct CleanupDriver {
    started: Rc<Cell<u32>>,
    completed_through: Rc<Cell<u32>>,
    fail_cleanup: Rc<Cell<Option<u32>>>,
    acquisitions: Rc<Cell<u32>>,
}

impl AcquisitionDriver<EnvironmentFields, EnvironmentSnapshot> for CleanupDriver {
    type Error = ();

    async fn acquire(
        &mut self,
        fields: EnvironmentFields,
    ) -> Result<(EnvironmentFields, EnvironmentSnapshot), Self::Error> {
        self.acquisitions.set(self.acquisitions.get() + 1);
        Ok((fields, EnvironmentSnapshot::default()))
    }

    async fn cancel(&mut self) -> Result<(), Self::Error> {
        let attempt = self.started.get() + 1;
        self.started.set(attempt);
        poll_fn(|_| {
            if self.completed_through.get() < attempt {
                Poll::Pending
            } else if self.fail_cleanup.get() == Some(attempt) {
                Poll::Ready(Err(()))
            } else {
                Poll::Ready(Ok(()))
            }
        })
        .await
    }

    fn min_acquisition_interval(&self) -> Duration {
        Duration::ZERO
    }
}

fn controlled_provider(driver: &CleanupDriver) -> ControlledProvider {
    ProviderLoop::new(
        driver.clone(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    )
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

fn immediately<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    match poll_once(future.as_mut()) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("expected immediate completion"),
    }
}

fn assert_running(provider: &mut ControlledProvider, driver: &CleanupDriver) {
    let _clock = clock();
    assert!(matches!(
        immediately(provider.step(&demand(), || Instant::ZERO)),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(driver.acquisitions.get(), 1);
}

#[test]
fn sleep_acknowledgement_belongs_to_the_exact_current_attempt() {
    let control = SleepControl::new();
    let first = control.request_suspend().unwrap();
    assert_eq!(
        immediately(control.wait_suspended(first, ready(()))),
        SuspendAck::TimedOut
    );
    // A completion may be retained after its caller's timeout, but a new
    // request must invalidate it before another waiter can observe success.
    assert!(control.acknowledge(first, SuspendOutcome::Quiesced));
    let resume = control.request_resume();
    let next = control.request_suspend().unwrap();
    assert_ne!(first.id(), next.id());
    assert_eq!(resume.command, Command::Resume);
    assert_eq!(next.command, Command::Suspend);
    assert!(control.is_current(next));
    assert!(!control.acknowledge(first, SuspendOutcome::Quiesced));
    assert!(!control.acknowledge(resume, SuspendOutcome::Quiesced));
    assert_eq!(
        immediately(control.wait_suspended(first, pending())),
        SuspendAck::Superseded
    );
    assert_eq!(
        immediately(control.wait_suspended(next, ready(()))),
        SuspendAck::TimedOut
    );
    assert_eq!(control.try_receive(), Some(next));
    assert!(control.acknowledge(next, SuspendOutcome::Quiesced));
    assert_eq!(
        immediately(control.wait_suspended(next, pending())),
        SuspendAck::Quiesced
    );
}

#[test]
fn late_cleanup_and_coalesced_resume_cannot_authorize_or_resume_retry() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    let mut provider = controlled_provider(&driver);
    let first = control.request_suspend().unwrap();
    assert_eq!(control.try_receive(), Some(first));
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", first, || Instant::ZERO)
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 1);
        assert_eq!(
            immediately(control.wait_suspended(first, ready(()))),
            SuspendAck::TimedOut
        );
        control.request_resume();
        let retry = control.request_suspend().unwrap();
        driver.completed_through.set(1);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 2);
        assert_eq!(
            immediately(control.wait_suspended(retry, ready(()))),
            SuspendAck::TimedOut,
            "the old cleanup must not authorize the retry"
        );
        driver.completed_through.set(2);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            immediately(control.wait_suspended(retry, pending())),
            SuspendAck::Quiesced
        );
        assert!(poll_once(actor.as_mut()).is_pending(), "no phantom resume");
        assert_eq!(driver.acquisitions.get(), 0);
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert!(control.try_receive().is_none());
    assert_running(&mut provider, &driver);
}

#[test]
fn latest_resume_recovers_after_wait_timeout_and_delayed_cleanup() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    let mut provider = controlled_provider(&driver);
    let request = control.request_suspend().unwrap();
    assert_eq!(control.try_receive(), Some(request));
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", request, || Instant::ZERO)
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            immediately(control.wait_suspended(request, ready(()))),
            SuspendAck::TimedOut
        );
        control.request_resume();
        driver.completed_through.set(1);
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert!(!control.acknowledge(request, SuspendOutcome::Quiesced));
    assert_eq!(driver.started.get(), 1);
    assert_running(&mut provider, &driver);
}

#[test]
fn new_suspend_retries_failed_cleanup_while_provider_remains_suspended() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    driver.completed_through.set(1);
    driver.fail_cleanup.set(Some(1));
    let mut provider = controlled_provider(&driver);
    let first = control.request_suspend().unwrap();
    assert_eq!(control.try_receive(), Some(first));
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", first, || Instant::ZERO)
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            immediately(control.wait_suspended(first, pending())),
            SuspendAck::CleanupFailed
        );
        let retry = control.request_suspend().unwrap();
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 2);
        assert_eq!(
            immediately(control.wait_suspended(retry, ready(()))),
            SuspendAck::TimedOut
        );
        driver.completed_through.set(2);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            immediately(control.wait_suspended(retry, pending())),
            SuspendAck::Quiesced
        );
        assert_eq!(driver.acquisitions.get(), 0);
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_running(&mut provider, &driver);
}

#[test]
fn synthetic_rejection_waits_for_real_cleanup_and_is_scoped_to_one_request() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    let mut provider = controlled_provider(&driver);
    let mut waiter = core::pin::pin!(control.suspend(&ingress, pending(), true));
    let request = control.try_receive().unwrap();
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", request, || Instant::ZERO)
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 1);
        assert!(poll_once(waiter.as_mut()).is_pending());
        assert_eq!(control.snapshot(), (request, None));
        driver.completed_through.set(1);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            poll_once(waiter.as_mut()),
            Poll::Ready(SuspendAck::CleanupFailed)
        );

        let mut retry = core::pin::pin!(control.suspend(&ingress, pending(), false));
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 2);
        assert!(poll_once(retry.as_mut()).is_pending());
        driver.completed_through.set(2);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(poll_once(retry.as_mut()), Poll::Ready(SuspendAck::Quiesced));
        assert_eq!(driver.acquisitions.get(), 0);
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_running(&mut provider, &driver);
}

#[test]
fn rejected_sleep_closes_periodic_override_and_resumes_latest_live_demand() {
    use observation::periodic::{DemandControl, PeriodicRequest, PeriodicStatus};
    let _clock = clock();
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let demands = DemandControl::<NoopRawMutex, EnvironmentFields, 2>::new();
    let driver = CleanupDriver::default();
    let mut provider = controlled_provider(&driver);
    demands.publish(Demand::uniform(
        EnvironmentFields::from_bits_truncate(3),
        Duration(60_000),
    ));
    assert!(matches!(
        immediately(provider.step(&demands.live(), now)),
        StepOutcome::Acquired { .. }
    ));
    demands
        .begin(
            PeriodicRequest {
                id: 1,
                interval: Duration(180_000),
                expires_at: Instant(600_000),
            },
            EnvironmentFields::from_bits_truncate(3),
            now(),
            ingress.close_generation(),
        )
        .unwrap();
    assert_eq!(
        demands
            .resolve(now(), ingress.close_generation())
            .event
            .unwrap()
            .status,
        PeriodicStatus::Applied
    );
    demands.publish(Demand::empty());
    let mut waiter = core::pin::pin!(control.suspend(&ingress, pending(), true));
    let request = control.try_receive().unwrap();
    // This is the settled boundary used by both production provider tasks.
    assert_eq!(
        demands.end(now(), false).unwrap().status,
        PeriodicStatus::Closed
    );
    assert!(!demands.is_pending());
    {
        let mut actor =
            core::pin::pin!(control.handle(&mut provider, &ingress, "TEST", request, now));
        assert!(poll_once(actor.as_mut()).is_pending());
        assert!(poll_once(waiter.as_mut()).is_pending());
        driver.completed_through.set(1);
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            poll_once(waiter.as_mut()),
            Poll::Ready(SuspendAck::CleanupFailed)
        );
        // A changed policy while parked must replace neither the old override
        // nor a saved pre-fixture live snapshot when recovery resumes.
        demands.publish(Demand::uniform(
            EnvironmentFields::HUMIDITY,
            Duration(120_000),
        ));
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    let restored = demands.resolve(now(), ingress.close_generation());
    assert!(restored.window.is_none());
    assert!(restored.event.is_none());
    assert_eq!(restored.demand.fields, EnvironmentFields::HUMIDITY);
    assert_eq!(restored.demand.max_age_at(1), Some(Duration(120_000)));
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(119_999));
    assert_eq!(
        immediately(provider.step(&restored.demand, now)),
        StepOutcome::Idle {
            next_wake: Some(Instant(120_000))
        }
    );
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(1));
    assert!(matches!(
        immediately(provider.step(&restored.demand, now)),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(driver.acquisitions.get(), 2);
}

#[test]
fn dropping_sleep_wait_preserves_intent_and_provider_recovery() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    driver.completed_through.set(1);
    let mut provider = controlled_provider(&driver);
    let request = control.request_suspend().unwrap();
    {
        let mut waiter = core::pin::pin!(control.wait_suspended(request, pending()));
        assert!(poll_once(waiter.as_mut()).is_pending());
    }
    assert_eq!(control.try_receive(), Some(request));
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", request, || Instant::ZERO)
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            immediately(control.wait_suspended(request, pending())),
            SuspendAck::Quiesced
        );
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_running(&mut provider, &driver);
}

#[test]
fn superseded_resume_cannot_reopen_provider_under_new_suspend() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    driver.completed_through.set(1);
    let mut provider = controlled_provider(&driver);
    let old_resume = control.request_resume();
    assert_eq!(control.try_receive(), Some(old_resume));
    let suspend = control.request_suspend().unwrap();
    {
        let mut actor =
            core::pin::pin!(
                control.handle(&mut provider, &ingress, "TEST", old_resume, || {
                    Instant::ZERO
                })
            );
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(driver.started.get(), 1);
        assert_eq!(
            immediately(control.wait_suspended(suspend, pending())),
            SuspendAck::Quiesced
        );
        assert_eq!(driver.acquisitions.get(), 0);
        control.request_resume();
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_running(&mut provider, &driver);
}

#[test]
fn resume_while_running_does_not_invent_a_cleanup_obligation() {
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    let mut provider = controlled_provider(&driver);
    let request = control.request_resume();
    assert_eq!(control.try_receive(), Some(request));
    immediately(control.handle(&mut provider, &ingress, "TEST", request, || Instant::ZERO));
    assert_eq!(driver.started.get(), 0);
    assert_running(&mut provider, &driver);
}

#[test]
fn only_current_quiescence_permits_sleep() {
    assert!(SuspendAck::Quiesced.permits_sleep());
    for outcome in [
        SuspendAck::CleanupFailed,
        SuspendAck::TimedOut,
        SuspendAck::Superseded,
        SuspendAck::Exhausted,
    ] {
        assert!(!outcome.permits_sleep());
    }
    assert_eq!(
        immediately(SleepControl::new().suspend(
            &RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new(),
            ready(()),
            false
        )),
        SuspendAck::TimedOut
    );
}

fn request<F: FieldMask>(expires_at: u64) -> ObserveNowRequest<F> {
    ObserveNowRequest {
        owner: OwnerId(1),
        owner_generation: OwnerGeneration(0),
        fields: F::from_bits_truncate(3),
        admitted_at: now(),
        expires_at: Instant(expires_at),
    }
}

#[test]
fn shtc3_request_during_conversion_uses_completed_sample_after_withdrawal() {
    let _clock = clock();
    let bus = Bus::default();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    let outcome = {
        let demand = demand();
        let mut conversion = core::pin::pin!(provider.step(&demand, now));
        assert!(poll_once(conversion.as_mut()).is_pending());
        embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(20));
        ingress.try_admit(request(500), now()).unwrap();
        run(conversion)
    };
    environment::control::drain_requests(&mut provider, &ingress, Some(outcome), now, "SHTC3");
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(
        bus.trace
            .borrow()
            .iter()
            .filter(|&&event| event == "read")
            .count(),
        1
    );
}

#[test]
fn adc_request_during_conversion_and_latest_demand_prevent_duplicate_read() {
    let _clock = clock();
    let adc = Adc::default();
    let ingress = RequestIngress::<NoopRawMutex, BatteryFields, 2>::new();
    let demand_watch =
        embassy_sync::watch::Watch::<NoopRawMutex, Demand<BatteryFields, 2>, 1>::new();
    let mut receiver = demand_watch.receiver().unwrap();
    demand_watch.sender().send(demand());
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        AdcBatteryDriver::new(adc.clone()),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        BatterySnapshot::default(),
    );
    let outcome = {
        let demand = receiver.try_get().unwrap_or_else(Demand::empty);
        let mut conversion = core::pin::pin!(provider.step(&demand, now));
        assert!(poll_once(conversion.as_mut()).is_pending());
        demand_watch.sender().send(Demand::empty());
        ingress.try_admit(request(500), now()).unwrap();
        adc.ready.set(true);
        run(conversion)
    };
    environment::control::drain_requests(&mut provider, &ingress, Some(outcome), now, "ADC");
    assert_eq!(ingress.outstanding(), 0);
    let demand = receiver.try_get().unwrap_or_else(Demand::empty);
    assert_eq!(
        run(provider.step(&demand, now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(adc.starts.get(), 1);
}

#[test]
fn accepted_adc_one_shot_survives_navigation_demand_withdrawal() {
    let _clock = clock();
    let adc = Adc::default();
    adc.ready.set(true);
    let ingress = RequestIngress::<NoopRawMutex, BatteryFields, 2>::new();
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        AdcBatteryDriver::new(adc.clone()),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        BatterySnapshot::default(),
    );
    ingress.try_admit(request(500), now()).unwrap();
    environment::control::drain_requests(&mut provider, &ingress, None, now, "ADC");
    assert_eq!(ingress.outstanding(), 1);
    let before = provider.pending_observe_now();
    assert!(matches!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::Acquired { .. }
    ));
    ingress.complete(before - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(adc.starts.get(), 1);
}

#[test]
fn shtc3_expiry_releases_credit_during_failed_read_backoff() {
    let _clock = clock();
    let bus = Bus::default();
    bus.acquisition.set(Fault::Error);
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    ingress.try_admit(request(71), now()).unwrap();
    environment::control::drain_requests(&mut provider, &ingress, None, now, "SHTC3");
    let before = provider.pending_observe_now();
    assert!(matches!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::AcquisitionFailed { .. }
    ));
    ingress.complete(before - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 1);
    assert_eq!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::Idle {
            next_wake: Some(Instant(71))
        }
    );
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(71 - now().0));
    let before = provider.pending_observe_now();
    assert_eq!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    ingress.complete(before - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 0);
    ingress.try_admit(request(500), now()).unwrap();
}

#[test]
fn expired_shtc3_arrival_during_conversion_is_discarded_without_extra_read() {
    let _clock = clock();
    let bus = Bus::default();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    let outcome = {
        let demand = demand();
        let mut conversion = core::pin::pin!(provider.step(&demand, now));
        assert!(poll_once(conversion.as_mut()).is_pending());
        ingress.try_admit(request(20), now()).unwrap();
        run(conversion)
    };
    environment::control::drain_requests(&mut provider, &ingress, Some(outcome), now, "SHTC3");
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(
        run(provider.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
}

#[test]
fn both_providers_close_full_ingress_before_waiting_and_clear_pending_before_ack() {
    let _clock = clock();
    let environment_control = SleepControl::new();
    let battery_control = SleepControl::new();
    let environment_ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let battery_ingress = RequestIngress::<NoopRawMutex, BatteryFields, 2>::new();
    let bus = Bus::default();
    let adc = Adc::default();
    let mut environment = ProviderLoop::<_, _, _, 2, 2>::new(
        sensor(&bus),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot::default(),
    );
    let mut battery = ProviderLoop::<_, _, _, 2, 2>::new(
        AdcBatteryDriver::new(adc.clone()),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        BatterySnapshot::default(),
    );
    environment_ingress.try_admit(request(500), now()).unwrap();
    battery_ingress.try_admit(request(500), now()).unwrap();
    environment::control::drain_requests(
        &mut environment,
        &environment_ingress,
        None,
        now,
        "SHTC3",
    );
    environment::control::drain_requests(&mut battery, &battery_ingress, None, now, "ADC");
    environment_ingress.try_admit(request(500), now()).unwrap();
    battery_ingress.try_admit(request(500), now()).unwrap();
    assert_eq!(
        environment_ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Full)
    );
    assert_eq!(
        battery_ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Full)
    );

    // These futures have not been polled: closure and intent are synchronous.
    let environment_wait = environment_control.suspend(&environment_ingress, pending(), false);
    let battery_wait = battery_control.suspend(&battery_ingress, pending(), false);
    assert_eq!(environment_ingress.outstanding(), 1);
    assert_eq!(battery_ingress.outstanding(), 1);
    assert_eq!(
        environment_ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    assert_eq!(
        battery_ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    let environment_request = environment_control.try_receive().unwrap();
    let battery_request = battery_control.try_receive().unwrap();
    {
        let mut environment_actor = core::pin::pin!(environment_control.handle(
            &mut environment,
            &environment_ingress,
            "SHTC3",
            environment_request,
            now
        ));
        let mut battery_actor = core::pin::pin!(battery_control.handle(
            &mut battery,
            &battery_ingress,
            "ADC",
            battery_request,
            now
        ));
        assert!(poll_once(environment_actor.as_mut()).is_pending());
        assert!(poll_once(battery_actor.as_mut()).is_pending());
        assert_eq!(immediately(environment_wait), SuspendAck::Quiesced);
        assert_eq!(immediately(battery_wait), SuspendAck::Quiesced);
        assert_eq!(environment_ingress.outstanding(), 0);
        assert_eq!(battery_ingress.outstanding(), 0);
        assert_eq!(adc.cancels.get(), 1);
        assert_eq!(adc.starts.get(), 0);
        environment_control.request_resume();
        battery_control.request_resume();
        assert!(poll_once(environment_actor.as_mut()).is_ready());
        assert!(poll_once(battery_actor.as_mut()).is_ready());
    }
    assert_eq!(environment.pending_observe_now(), 0);
    assert_eq!(battery.pending_observe_now(), 0);
    assert_eq!(bus.trace.borrow().as_slice(), ["sleep"]);
    environment_ingress.try_admit(request(500), now()).unwrap();
    battery_ingress.try_admit(request(500), now()).unwrap();
}

#[test]
fn terminal_provider_keeps_ingress_closed_across_resume_but_still_acknowledges_sleep() {
    let _clock = clock();
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    driver.completed_through.set(1);
    let mut provider = controlled_provider(&driver);
    let mut actor = core::pin::pin!(control.serve_stopped(&mut provider, &ingress, "TEST", now));
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    control.request_resume();
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    let wait = control.suspend(&ingress, pending(), false);
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(immediately(wait), SuspendAck::Quiesced);
    control.request_resume();
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    assert_eq!(driver.acquisitions.get(), 0);
    assert_eq!(driver.started.get(), 1);
}

#[test]
fn failed_cleanup_returns_pending_credit_but_resume_is_required_to_reopen() {
    let _clock = clock();
    let control = SleepControl::new();
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 1>::new();
    let driver = CleanupDriver::default();
    driver.completed_through.set(1);
    driver.fail_cleanup.set(Some(1));
    let mut provider = controlled_provider(&driver);
    ingress.try_admit(request(500), now()).unwrap();
    environment::control::drain_requests(&mut provider, &ingress, None, now, "TEST");
    let wait = control.suspend(&ingress, pending(), false);
    let request_id = control.try_receive().unwrap();
    let mut actor =
        core::pin::pin!(control.handle(&mut provider, &ingress, "TEST", request_id, now));
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(immediately(wait), SuspendAck::CleanupFailed);
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    control.request_resume();
    assert!(poll_once(actor.as_mut()).is_ready());
    ingress.try_admit(request(500), now()).unwrap();
}

#[test]
fn two_provider_sleep_waiters_share_one_deadline_and_retain_timed_out_intent() {
    let _clock = clock();
    let environment_control = SleepControl::new();
    let battery_control = SleepControl::new();
    let environment_ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let battery_ingress = RequestIngress::<NoopRawMutex, BatteryFields, 2>::new();
    let deadline = embassy_time::Instant::from_millis(2_000);
    let environment_wait = environment_control.suspend(
        &environment_ingress,
        embassy_time::Timer::at(deadline),
        false,
    );
    let battery_wait =
        battery_control.suspend(&battery_ingress, embassy_time::Timer::at(deadline), false);
    let mut wait = core::pin::pin!(embassy_futures::join::join(environment_wait, battery_wait));
    assert!(poll_once(wait.as_mut()).is_pending());
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(2_000));
    assert_eq!(
        poll_once(wait.as_mut()),
        Poll::Ready((SuspendAck::TimedOut, SuspendAck::TimedOut))
    );
    assert_eq!(now(), Instant(2_000));
    let environment_request = environment_control.try_receive().unwrap();
    let battery_request = battery_control.try_receive().unwrap();
    assert_eq!(environment_request.command, Command::Suspend);
    assert_eq!(battery_request.command, Command::Suspend);
    assert_eq!(
        environment_ingress.try_admit(request(2_500), now()),
        Err(RequestAdmissionError::Closed)
    );
    assert_eq!(
        battery_ingress.try_admit(request(2_500), now()),
        Err(RequestAdmissionError::Closed)
    );
    environment_control.request_resume();
    battery_control.request_resume();
    let _environment_retry = environment_control.suspend(&environment_ingress, pending(), false);
    let _battery_retry = battery_control.suspend(&battery_ingress, pending(), false);
    assert!(!environment_control.acknowledge(environment_request, SuspendOutcome::Quiesced));
    assert!(!battery_control.acknowledge(battery_request, SuspendOutcome::Quiesced));
}

#[test]
fn adc_entry_refresh_uses_successful_revision_when_two_samples_share_a_millisecond() {
    use medinote::observations::{BatteryStateSnapshot, EntryAdmissionEvent, HomeObservations};
    use shell::types::{RefreshHint, SurfaceCapabilities, SurfaceRole, SurfaceSpec};
    const HOME: &[SurfaceSpec] = &[SurfaceSpec::new(
        medinote::catalogue::HOME_SURFACE_ID.0,
        SurfaceRole::Ambient,
        SurfaceCapabilities::AMBIENT,
        RefreshHint::Content,
    )];
    for succeeds in [true, false] {
        let _clock = clock();
        let adc = Adc::default();
        adc.ready.set(true);
        let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
            AdcBatteryDriver::new(adc.clone()),
            medinote::observations::BATTERY_PROVIDER_ID,
            ProviderGeneration::INITIAL,
            BatterySnapshot::default(),
        );
        let StepOutcome::Acquired { revision, .. } = run(provider.step(&demand(), now)) else {
            panic!("initial ADC sample failed");
        };
        let mut last_sample_revision = Some(revision);
        let state = provider.state();
        let baseline = BatteryStateSnapshot::from_observation(state, last_sample_revision);
        let shell = shell::model::ShellModel::<1, 1, 1, 1, 1, 1>::new(
            medinote::catalogue::BASE_PROVIDER_ID,
            HOME,
            medinote::catalogue::HOME_SURFACE_ID,
        )
        .unwrap();
        let mut home = HomeObservations::new();
        home.committed(shell.active_instance(), now()).unwrap();
        let key = home.battery_key().unwrap();
        let ingress = RequestIngress::<NoopRawMutex, BatteryFields, 2>::new();
        assert_eq!(
            home.admit_battery(
                now(),
                Some(baseline),
                ingress.close_generation(),
                |request, at| { ingress.try_admit(request, at) }
            ),
            Some(EntryAdmissionEvent::Admitted)
        );
        assert!(home
            .poll_battery(key, baseline, now())
            .unwrap()
            .sample
            .is_some());
        environment::control::drain_requests(&mut provider, &ingress, None, now, "ADC");
        adc.ready.set(succeeds);
        let before = provider.pending_observe_now();
        let outcome = run(provider.step(&Demand::empty(), now));
        if let StepOutcome::Acquired { revision, .. } = outcome {
            last_sample_revision = Some(revision);
        }
        ingress.complete(before - provider.pending_observe_now());
        let state = provider.state();
        let completed = BatteryStateSnapshot::from_observation(state, last_sample_revision);
        assert_ne!(completed.revision, baseline.revision);
        assert_eq!(completed.last_sample_at, baseline.last_sample_at);
        assert_eq!(
            home.admit_battery(
                now(),
                Some(completed),
                ingress.close_generation(),
                |_, _| { panic!("the admitted entry must not issue a duplicate request") }
            ),
            None
        );
        let delivery = home.poll_battery(key, completed, now());
        if succeeds {
            assert_eq!(
                now(),
                Instant(0),
                "both real ADC conversions completed in one millisecond"
            );
            assert_ne!(
                completed.last_sample_revision,
                baseline.last_sample_revision
            );
            assert!(
                delivery.unwrap().sample.is_some(),
                "fresh entry bypasses the minute delivery interval"
            );
            assert_eq!(ingress.outstanding(), 0);
        } else {
            assert_eq!(
                completed.last_sample_revision,
                baseline.last_sample_revision
            );
            let delivery = delivery.unwrap();
            // Health transitions may redeliver available cache, but they must
            // not complete this entry's explicit refresh obligation.
            assert!(delivery.health.is_some());
            assert_eq!(ingress.outstanding(), 1);
            let control = SleepControl::new();
            let waiter = control.suspend(&ingress, pending(), false);
            let request = control.try_receive().unwrap();
            {
                let mut actor =
                    core::pin::pin!(control.handle(&mut provider, &ingress, "ADC", request, now));
                assert!(poll_once(actor.as_mut()).is_pending());
                assert_eq!(immediately(waiter), SuspendAck::Quiesced);
                control.request_resume();
                assert!(poll_once(actor.as_mut()).is_ready());
            }
            assert_eq!(
                home.admit_battery(
                    now(),
                    Some(completed),
                    ingress.close_generation(),
                    |request, at| { ingress.try_admit(request, at) }
                ),
                Some(EntryAdmissionEvent::Admitted),
                "failed acquisition leaves the entry live for readmission after closure"
            );
        }
    }
}
