//! Deterministic host tests closing the battery diagnostic coverage gap
//! (ADR-0018 plan, Phase 5: "Exercise wake failure, read failure, invalid
//! percentage, repeated failure, and subsequent recovery through the
//! actual acquisition path. Verify retained level and exactly one
//! diagnostic per failed attempt. The existing 55 tests don't exercise
//! that logging path.").
//!
//! `platform/sensing/observation`'s 55 tests exercise `ProviderLoop`'s generic
//! retention/backoff/revision machinery against synthetic `FakeDriver`
//! stand-ins -- real, but not `battery`'s own driver. `battery::driver`
//! (formerly inline in `battery.rs`) is generic over
//! `embedded_hal_async::i2c::I2c`, its own diagnostics sink, and its wake
//! operation precisely so the *actual* `BqDriver::acquire()` body -- wake,
//! read, range-check, and the single `diagnostics.acquire_failed()` call
//! site -- can be driven here through a real `ProviderLoop`, with only the
//! I2C bus, the wake step, and the diagnostics sink faked. The wake step is
//! faked to isolate driver error/diagnostic behavior. The shared expander
//! has separate actual-owner reconciliation tests in the board crate. This
//! narrow `FuelGaugeWake` boundary makes the rest of the
//! acquisition path testable at all without that dependency. Path-included
//! the same way `bounded_control.rs`/`panel_refresh_tracking.rs` already
//! pull in their production sources; `driver.rs`'s own `super::types`
//! resolves correctly because `types` is declared as the same kind of
//! sibling module here.

#[path = "../../../products/meditamer/src/firmware/battery/driver.rs"]
mod driver;
#[path = "../../../products/meditamer/src/firmware/battery/types.rs"]
mod types;

#[allow(dead_code)]
#[path = "../../../products/meditamer/src/firmware/bounded_control.rs"]
mod bounded_control;
mod firmware {
    pub(crate) use crate::bounded_control;
}
#[path = "../../../products/meditamer/src/firmware/battery/runtime.rs"]
mod runtime;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use embedded_hal_async::i2c::{ErrorKind, ErrorType, I2c, Operation};

use observation::demand::{aggregate_demand, Demand};
use observation::ids::{OwnerGeneration, OwnerId, ProviderGeneration, SubscriptionSlot};
use observation::policy::InitialPolicy;
use observation::runtime::{ProviderLoop, StepOutcome};
use observation::subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey};
use observation::time::{Duration as ObsDuration, Instant as ObsInstant};

use driver::{BatteryDiagnostics, BatteryError, BqDriver, FuelGaugeWake};
use types::{BatteryFields, BatterySnapshot, BATTERY_PROVIDER_ID, FIELDS};

/// No-op-waker poll loop -- `platform/sensing/observation`'s own pattern
/// (`platform/sensing/observation/src/runtime.rs`'s test module) for host-testing
/// simple futures without a real executor. Nothing here awaits a real
/// timer (unlike production's `PcalExpander::wake_fuel_gauge`, which this
/// test's `FakeWake` does not route through), so this loop's busy-poll
/// never actually spins waiting on real wall time.
fn block_on<Fut: core::future::Future>(future: Fut) -> Fut::Output {
    let mut future = core::pin::pin!(future);
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    loop {
        if let core::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FakeI2cError;

impl embedded_hal_async::i2c::Error for FakeI2cError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// A fake I2C device shared (via `Rc<Cell<_>>`) with the test so it can
/// flip `fail`/`response` between `ProviderLoop::step` calls -- stands in
/// for `BqDriver`'s own fuel-gauge read handle, matching production's
/// `SharedI2cDevice`.
#[derive(Clone)]
struct FakeI2c {
    fail: Rc<Cell<bool>>,
    response: Rc<Cell<[u8; 2]>>,
}

impl FakeI2c {
    fn new(fail: Rc<Cell<bool>>, response: Rc<Cell<[u8; 2]>>) -> Self {
        Self { fail, response }
    }
}

impl ErrorType for FakeI2c {
    type Error = FakeI2cError;
}

impl I2c for FakeI2c {
    async fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        if self.fail.get() {
            return Err(FakeI2cError);
        }
        for op in operations {
            if let Operation::Read(buffer) = op {
                let response = self.response.get();
                buffer.copy_from_slice(&response[..buffer.len()]);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FakeWakeError;

/// A fake wake operation shared (via `Rc<Cell<bool>>`) with the test so it
/// can flip `fail` between `ProviderLoop::step` calls -- stands in for
/// production's `SharedExpanderWake`, which locks the real shared
/// `PcalExpander` and calls its `wake_fuel_gauge`; see this file's own top
/// doc comment for why that real path cannot be used here.
#[derive(Clone)]
struct FakeWake {
    fail: Rc<Cell<bool>>,
    delay_ms: Rc<Cell<u64>>,
    calls: Rc<Cell<usize>>,
}

impl FuelGaugeWake for FakeWake {
    type Error = FakeWakeError;

    async fn wake_fuel_gauge(&mut self) -> Result<(), Self::Error> {
        self.calls.set(self.calls.get() + 1);
        if self.delay_ms.get() > 0 {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(self.delay_ms.get()))
                .await;
        }
        if self.fail.get() {
            Err(FakeWakeError)
        } else {
            Ok(())
        }
    }
}

/// Records every failed attempt as a formatted string -- "exactly one
/// diagnostic per failed attempt" is asserted by checking `log`'s length
/// after each step, and *which* error fired by checking its content,
/// against the real `Debug` formatting `console::println!` would have used
/// in production (`ConsoleDiagnostics` in `battery/mod.rs`, which this
/// mirrors except for where the message goes).
#[derive(Clone, Default)]
struct CountingDiagnostics {
    log: Rc<RefCell<Vec<String>>>,
}

impl<E: core::fmt::Debug> BatteryDiagnostics<E> for CountingDiagnostics {
    fn acquire_failed(&mut self, error: &BatteryError<E>) {
        self.log.borrow_mut().push(format!("{error:?}"));
    }
}

fn subscription() -> ObservationSubscription<BatteryFields> {
    ObservationSubscription {
        key: SubscriptionKey {
            provider: BATTERY_PROVIDER_ID,
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: BatteryFields::LEVEL,
        max_age: ObsDuration::ZERO, // always due, simplest for these tests
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: ObsDuration::ZERO,
        expires_at: None,
    }
}

fn demand_at(t: u64) -> Demand<BatteryFields, FIELDS> {
    let mut book: SubscriptionBook<BatteryFields, 1> = SubscriptionBook::new();
    book.upsert(subscription())
        .expect("the one test subscription fits its capacity-1 book");
    aggregate_demand(&book, BATTERY_PROVIDER_ID, ObsInstant(t))
}

/// Everything a test needs: the shared `Rc<Cell<_>>` handles let a test
/// flip failure/response conditions *between* `ProviderLoop::step` calls
/// on the one `BqDriver` `provider()` builds, without needing to rebuild
/// the driver (which would lose `ProviderLoop`'s own retained state) each
/// time.
struct Harness {
    expander_fail: Rc<Cell<bool>>,
    fuelgauge_fail: Rc<Cell<bool>>,
    fuelgauge_response: Rc<Cell<[u8; 2]>>,
    diagnostics: CountingDiagnostics,
    wake_delay_ms: Rc<Cell<u64>>,
    wake_calls: Rc<Cell<usize>>,
}

impl Harness {
    fn new() -> Self {
        Self {
            expander_fail: Rc::new(Cell::new(false)),
            fuelgauge_fail: Rc::new(Cell::new(false)),
            fuelgauge_response: Rc::new(Cell::new([0u8, 0u8])),
            diagnostics: CountingDiagnostics::default(),
            wake_delay_ms: Rc::new(Cell::new(0)),
            wake_calls: Rc::new(Cell::new(0)),
        }
    }

    fn provider(
        &self,
    ) -> ProviderLoop<
        BatteryFields,
        BatterySnapshot,
        BqDriver<FakeI2c, FakeWake, CountingDiagnostics>,
        1,
        FIELDS,
    > {
        ProviderLoop::new(
            self.driver(),
            BATTERY_PROVIDER_ID,
            ProviderGeneration::INITIAL,
            BatterySnapshot::default(),
        )
    }

    fn driver(&self) -> BqDriver<FakeI2c, FakeWake, CountingDiagnostics> {
        BqDriver::new(
            FakeI2c::new(self.fuelgauge_fail.clone(), self.fuelgauge_response.clone()),
            FakeWake {
                fail: self.expander_fail.clone(),
                delay_ms: self.wake_delay_ms.clone(),
                calls: self.wake_calls.clone(),
            },
            self.diagnostics.clone(),
        )
    }

    fn diagnostics_log(&self) -> Vec<String> {
        self.diagnostics.log.borrow().clone()
    }
}

#[test]
fn wake_failure_produces_one_diagnostic_and_no_snapshot_update() {
    let harness = Harness::new();
    harness.expander_fail.set(true);
    let mut provider = harness.provider();

    let outcome = block_on(provider.step(&demand_at(0), || ObsInstant(0)));
    assert!(
        matches!(outcome, StepOutcome::AcquisitionFailed { .. }),
        "expected a failed acquisition, got {outcome:?}"
    );
    assert_eq!(provider.state().snapshot, BatterySnapshot::default());
    let log = harness.diagnostics_log();
    assert_eq!(
        log.len(),
        1,
        "exactly one diagnostic for the one failed attempt"
    );
    assert!(
        log[0].contains("WakeFailed"),
        "expected a WakeFailed diagnostic, got {log:?}"
    );
}

#[test]
fn read_failure_produces_one_diagnostic_and_no_snapshot_update() {
    let harness = Harness::new();
    harness.fuelgauge_fail.set(true);
    let mut provider = harness.provider();

    let outcome = block_on(provider.step(&demand_at(0), || ObsInstant(0)));
    assert!(
        matches!(outcome, StepOutcome::AcquisitionFailed { .. }),
        "expected a failed acquisition, got {outcome:?}"
    );
    assert_eq!(provider.state().snapshot, BatterySnapshot::default());
    let log = harness.diagnostics_log();
    assert_eq!(
        log.len(),
        1,
        "exactly one diagnostic for the one failed attempt"
    );
    assert!(
        log[0].contains("Read("),
        "expected a Read diagnostic, got {log:?}"
    );
}

#[test]
fn invalid_percentage_is_rejected_not_clamped_and_produces_one_diagnostic() {
    let harness = Harness::new();
    // 250 percent (little-endian, matching `read_state_of_charge`'s own
    // decode): out of range, and deliberately not close to either bound,
    // so this cannot pass by accidentally landing on a clamp.
    harness.fuelgauge_response.set([250, 0]);
    let mut provider = harness.provider();

    let outcome = block_on(provider.step(&demand_at(0), || ObsInstant(0)));
    assert!(
        matches!(outcome, StepOutcome::AcquisitionFailed { .. }),
        "expected a failed acquisition, got {outcome:?}"
    );
    assert_eq!(
        provider.state().snapshot,
        BatterySnapshot::default(),
        "an out-of-range reading must be discarded, not clamped into the snapshot"
    );
    let log = harness.diagnostics_log();
    assert_eq!(
        log.len(),
        1,
        "exactly one diagnostic for the one failed attempt"
    );
    assert!(
        log[0].contains("OutOfRange(250)"),
        "expected an OutOfRange(250) diagnostic, got {log:?}"
    );
}

#[test]
fn repeated_failure_retains_the_last_good_level_then_recovers() {
    let harness = Harness::new();
    let mut provider = harness.provider();

    // A successful acquisition at t=0: percent 84, no diagnostic.
    harness.fuelgauge_response.set([84, 0]);
    let outcome = block_on(provider.step(&demand_at(0), || ObsInstant(0)));
    assert!(matches!(outcome, StepOutcome::Acquired { .. }));
    assert_eq!(provider.state().snapshot.percent, 84);
    assert_eq!(
        harness.diagnostics_log().len(),
        0,
        "no diagnostic on success"
    );

    // `min_acquisition_interval` is 2s; the next attempt cannot start
    // before t=2000.
    harness.fuelgauge_fail.set(true);
    let outcome = block_on(provider.step(&demand_at(2_000), || ObsInstant(2_000)));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = outcome else {
        panic!("expected a failed acquisition, got {outcome:?}");
    };
    assert_eq!(
        provider.state().snapshot.percent,
        84,
        "the last good level must be retained across a failed attempt"
    );
    assert_eq!(harness.diagnostics_log().len(), 1);

    // Retry backoff after one failure doubles from the minimum interval
    // (2s -> 4s), measured from completion: t=2000 + 4000 = 6000.
    // Start spacing is a separate bound, not added again to that backoff.
    // A second failure in the wake step exercises a distinct failed attempt.
    assert_eq!(retry_at, ObsInstant(6_000));
    harness.fuelgauge_fail.set(false);
    harness.expander_fail.set(true);
    let outcome = block_on(provider.step(&demand_at(retry_at.0), || retry_at));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = outcome else {
        panic!("expected a second failed acquisition, got {outcome:?}");
    };
    assert_eq!(
        provider.state().snapshot.percent,
        84,
        "the last good level must survive a second, independent failure too"
    );
    let log = harness.diagnostics_log();
    assert_eq!(
        log.len(),
        2,
        "exactly one new diagnostic for the second failed attempt"
    );
    assert!(log[0].contains("Read("));
    assert!(log[1].contains("WakeFailed"));

    // Backoff after two consecutive failures doubles again (4s -> 8s), so
    // the next attempt cannot start before t=6000 + 8000 = 14000.
    // Recovery delivers a fresh percentage without another diagnostic.
    assert_eq!(retry_at, ObsInstant(14_000));
    harness.expander_fail.set(false);
    harness.fuelgauge_response.set([91, 0]);
    let outcome = block_on(provider.step(&demand_at(retry_at.0), || retry_at));
    assert!(
        matches!(outcome, StepOutcome::Acquired { .. }),
        "expected recovery to succeed, got {outcome:?}"
    );
    assert_eq!(provider.state().snapshot.percent, 91);
    assert_eq!(
        harness.diagnostics_log().len(),
        2,
        "no new diagnostic on the recovering, successful attempt"
    );
}

#[test]
fn idle_and_backoff_steps_do_not_acquire_or_repeat_diagnostics() {
    let harness = Harness::new();
    let mut provider = harness.provider();
    harness.fuelgauge_response.set([84, 0]);
    assert!(matches!(
        block_on(provider.step(&demand_at(0), || ObsInstant(0))),
        StepOutcome::Acquired { .. }
    ));
    harness.fuelgauge_fail.set(true);
    for at in [1, 1_999] {
        assert!(matches!(
            block_on(provider.step(&demand_at(at), || ObsInstant(at))),
            StepOutcome::Idle { .. }
        ));
        assert!(harness.diagnostics_log().is_empty());
    }
    let outcome = block_on(provider.step(&demand_at(2_000), || ObsInstant(2_000)));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = outcome else {
        panic!("expected failed acquisition, got {outcome:?}");
    };
    assert_eq!(retry_at, ObsInstant(6_000));
    let revision = provider.state().revision;
    for at in [2_001, 4_000, retry_at.0 - 1] {
        assert_eq!(
            block_on(provider.step(&demand_at(at), || ObsInstant(at))),
            StepOutcome::Idle {
                next_wake: Some(retry_at)
            }
        );
        assert_eq!(provider.state().revision, revision);
        assert_eq!(provider.state().snapshot.percent, 84);
        assert_eq!(harness.diagnostics_log().len(), 1);
    }
    harness.fuelgauge_fail.set(false);
    harness.fuelgauge_response.set([91, 0]);
    assert!(matches!(
        block_on(provider.step(&demand_at(retry_at.0), || retry_at)),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(provider.state().snapshot.percent, 91);
    assert_eq!(harness.diagnostics_log().len(), 1);
}

use bounded_control::{Control, ControlAck, SuspendAck, WaitOutcome};
use core::{
    future::{pending, ready, Future},
    pin::Pin,
    task::{Context, Poll, Waker},
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use observation::ingress::{RequestAdmissionError, RequestIngress};
use observation::observe_now::ObserveNowRequest;
use runtime::BatteryRuntime;
use std::sync::{Mutex as StdMutex, MutexGuard};

type Ingress = RequestIngress<NoopRawMutex, BatteryFields, 2>;
static CLOCK: StdMutex<()> = StdMutex::new(());

fn clock() -> MutexGuard<'static, ()> {
    let guard = CLOCK.lock().unwrap_or_else(|poison| poison.into_inner());
    embassy_time::MockDriver::get().reset();
    guard
}

fn now() -> ObsInstant {
    ObsInstant(embassy_time::Instant::now().as_millis())
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

fn run<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    for _ in 0..1_000 {
        if let Poll::Ready(value) = poll_once(future.as_mut()) {
            return value;
        }
        embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(1));
    }
    panic!("battery test exceeded virtual-clock operation bound");
}

fn request(expiry: u64) -> ObserveNowRequest<BatteryFields> {
    ObserveNowRequest {
        owner: OwnerId(1),
        owner_generation: OwnerGeneration(0),
        fields: BatteryFields::LEVEL,
        admitted_at: now(),
        expires_at: ObsInstant(expiry),
    }
}

#[test]
fn runtime_no_demand_is_idle_and_accepted_request_survives_withdrawal() {
    let _clock = clock();
    let harness = Harness::new();
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    assert_eq!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(harness.wake_calls.get(), 0);
    assert_eq!(runtime.state().sample_identity(), None);
    ingress.try_admit(request(500), now()).unwrap();
    runtime.drain_requests(None, now());
    assert!(matches!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(harness.wake_calls.get(), 1);
    let state = runtime.state();
    assert_eq!(state.provider, BATTERY_PROVIDER_ID);
    assert_eq!(state.last_attempt_at, Some(now()));
    assert_eq!(state.last_sample_at, Some(now()));
    assert_eq!(
        state.sample_identity(),
        Some((state.generation, state.revision))
    );
    assert_eq!(state.health, observation::policy::Health::Ok);
    assert_eq!(types::SUBSCRIPTION_CAPACITY, 2);
}

#[test]
fn runtime_arrival_during_real_wake_completes_without_second_acquisition() {
    let _clock = clock();
    let harness = Harness::new();
    harness.wake_delay_ms.set(20);
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    let outcome = {
        let demand = demand_at(0);
        let mut step = core::pin::pin!(runtime.step(&demand, now));
        assert!(poll_once(step.as_mut()).is_pending());
        ingress.try_admit(request(500), now()).unwrap();
        run(step)
    };
    runtime.drain_requests(Some(outcome), now());
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(runtime.state().last_sample_at, Some(ObsInstant(20)));
    assert_eq!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(harness.wake_calls.get(), 1);
}

#[test]
fn runtime_failure_retains_success_identity_and_expires_requests_before_backoff() {
    let _clock = clock();
    let harness = Harness::new();
    harness.fuelgauge_response.set([84, 0]);
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    run(runtime.step(&demand_at(0), now));
    let baseline = runtime.state();
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(2_000));
    harness.fuelgauge_fail.set(true);
    ingress.try_admit(request(2_100), now()).unwrap();
    runtime.drain_requests(None, now());
    let outcome = run(runtime.step(&Demand::empty(), now));
    assert!(matches!(
        outcome,
        StepOutcome::AcquisitionFailed {
            retry_at: ObsInstant(6_000),
            ..
        }
    ));
    let failed = runtime.state();
    assert_eq!(failed.snapshot, baseline.snapshot);
    assert_eq!(failed.last_sample_at, baseline.last_sample_at);
    assert_eq!(failed.sample_identity(), baseline.sample_identity());
    assert_ne!(failed.revision, baseline.revision);
    assert_eq!(failed.last_attempt_at, Some(ObsInstant(2_000)));
    assert_eq!(ingress.outstanding(), 1);
    assert_eq!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Idle {
            next_wake: Some(ObsInstant(2_100))
        }
    );
    embassy_time::MockDriver::get().advance(embassy_time::Duration::from_millis(100));
    assert_eq!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(harness.diagnostics_log().len(), 1);
    assert_eq!(harness.wake_calls.get(), 2);
}

#[test]
fn runtime_full_requests_do_not_delay_control_and_only_current_resume_reopens() {
    let _clock = clock();
    let harness = Harness::new();
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    ingress.try_admit(request(500), now()).unwrap();
    runtime.drain_requests(None, now());
    ingress.try_admit(request(500), now()).unwrap();
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Full)
    );
    let old_resume = control.request_resume(false);
    let suspend = control.request_suspend().unwrap();
    ingress.close();
    assert_eq!(ingress.outstanding(), 1);
    {
        let mut actor = core::pin::pin!(runtime.apply_control(old_resume, now));
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            run(control.wait_suspended(suspend, pending())),
            SuspendAck::Quiesced
        );
        assert_eq!(ingress.outstanding(), 0);
        assert_eq!(
            ingress.try_admit(request(500), now()),
            Err(RequestAdmissionError::Closed)
        );
        let resume = control.request_resume(false);
        assert!(poll_once(actor.as_mut()).is_ready());
        assert_eq!(
            run(control.wait(resume, ready(()))),
            WaitOutcome::Acknowledged(ControlAck::Running)
        );
    }
    ingress.try_admit(request(500), now()).unwrap();
    assert_eq!(harness.wake_calls.get(), 0);
}

#[test]
fn runtime_terminal_transition_precedes_queued_resume_and_preserves_sleep_control() {
    let _clock = clock();
    let harness = Harness::new();
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    let resume = control.request_resume(false);
    // Exercise the production post-step transition without iterating u32::MAX
    // samples; atomic counter exhaustion is covered in observation state tests.
    runtime.record_outcome(StepOutcome::RevisionExhausted);
    assert!(runtime.is_stopped());
    run(runtime.apply_control(resume, now));
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    assert_eq!(
        run(runtime.step(&demand_at(0), now)),
        StepOutcome::RevisionExhausted
    );
    let suspend = control.request_suspend().unwrap();
    {
        let mut actor = core::pin::pin!(runtime.apply_control(suspend, now));
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            run(control.wait_suspended(suspend, pending())),
            SuspendAck::Quiesced
        );
        control.request_resume(false);
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    assert_eq!(harness.wake_calls.get(), 0);
}

#[derive(Clone, Default)]
struct ControlledCleanup {
    started: Rc<Cell<usize>>,
    finished: Rc<Cell<usize>>,
    failing: Rc<Cell<Option<usize>>>,
    acquisitions: Rc<Cell<usize>>,
}

impl observation::runtime::AcquisitionDriver<BatteryFields, BatterySnapshot> for ControlledCleanup {
    type Error = ();
    async fn acquire(
        &mut self,
        fields: BatteryFields,
    ) -> Result<(BatteryFields, BatterySnapshot), ()> {
        self.acquisitions.set(self.acquisitions.get() + 1);
        Ok((fields, BatterySnapshot::default()))
    }
    async fn cancel(&mut self) -> Result<(), ()> {
        let attempt = self.started.get() + 1;
        self.started.set(attempt);
        core::future::poll_fn(|_| {
            if self.finished.get() < attempt {
                Poll::Pending
            } else if self.failing.get() == Some(attempt) {
                Poll::Ready(Err(()))
            } else {
                Poll::Ready(Ok(()))
            }
        })
        .await
    }
    fn min_acquisition_interval(&self) -> ObsDuration {
        ObsDuration::ZERO
    }
}

#[test]
fn runtime_delayed_cleanup_cannot_acknowledge_newer_suspend_and_retry_cleans_again() {
    let _clock = clock();
    let driver = ControlledCleanup::default();
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(driver.clone(), &ingress, &control);
    ingress.try_admit(request(500), now()).unwrap();
    runtime.drain_requests(None, now());
    let first = control.request_suspend().unwrap();
    assert_eq!(control.try_receive(), Some(first));
    let mut actor = core::pin::pin!(runtime.apply_control(first, now));
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(driver.started.get(), 1);
    assert_eq!(
        ingress.outstanding(),
        1,
        "received credit remains held until cleanup returns"
    );
    assert_eq!(
        run(control.wait_suspended(first, ready(()))),
        SuspendAck::TimedOut
    );
    control.request_resume(false);
    let retry = control.request_suspend().unwrap();
    driver.finished.set(1);
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(driver.started.get(), 2);
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(
        run(control.wait_suspended(retry, ready(()))),
        SuspendAck::TimedOut
    );
    assert_eq!(
        ingress.try_admit(request(500), now()),
        Err(RequestAdmissionError::Closed)
    );
    driver.failing.set(Some(2));
    driver.finished.set(2);
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        run(control.wait_suspended(retry, pending())),
        SuspendAck::CleanupFailed
    );
    let third = control.request_suspend().unwrap();
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        driver.started.get(),
        3,
        "failed cleanup must be retried for new suspend"
    );
    driver.finished.set(3);
    assert!(poll_once(actor.as_mut()).is_pending());
    assert_eq!(
        run(control.wait_suspended(third, pending())),
        SuspendAck::Quiesced
    );
    control.request_resume(false);
    assert!(poll_once(actor.as_mut()).is_ready());
    ingress.try_admit(request(500), now()).unwrap();
    assert_eq!(driver.acquisitions.get(), 0);
}

#[test]
fn runtime_suspend_arriving_during_wake_is_serviced_before_another_read() {
    let _clock = clock();
    let harness = Harness::new();
    harness.wake_delay_ms.set(20);
    let ingress = Ingress::new();
    let control = Control::new();
    let mut runtime = BatteryRuntime::new(harness.driver(), &ingress, &control);
    let suspend;
    {
        let demand = demand_at(0);
        let mut step = core::pin::pin!(runtime.step(&demand, now));
        assert!(poll_once(step.as_mut()).is_pending());
        ingress.try_admit(request(500), now()).unwrap();
        ingress.close();
        suspend = control.request_suspend().unwrap();
        assert_eq!(
            run(control.wait_suspended(suspend, ready(()))),
            SuspendAck::TimedOut
        );
        assert!(matches!(run(step), StepOutcome::Acquired { .. }));
    }
    assert_eq!(control.try_receive(), Some(suspend));
    {
        let mut actor = core::pin::pin!(runtime.apply_control(suspend, now));
        assert!(poll_once(actor.as_mut()).is_pending());
        assert_eq!(
            run(control.wait_suspended(suspend, pending())),
            SuspendAck::Quiesced
        );
        assert_eq!(ingress.outstanding(), 0);
        assert_eq!(harness.wake_calls.get(), 1);
        control.request_resume(false);
        assert!(poll_once(actor.as_mut()).is_ready());
    }
    assert_eq!(
        run(runtime.step(&Demand::empty(), now)),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(harness.wake_calls.get(), 1);
}
