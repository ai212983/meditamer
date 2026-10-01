use super::*;
use crate::demand::aggregate_demand;
use crate::field::test_fields::TestFields;
use crate::ids::{OwnerGeneration, OwnerId, SubscriptionSlot};
use crate::observe_now::ObserveNowRequest;
use crate::policy::{Health, InitialPolicy};
use crate::subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey};

/// No-op-waker poll loop -- the fakes below never actually pend, so no
/// executor dependency is needed (`platform/time/wall-clock`'s
/// `tests/support` pattern; `embassy_futures::block_on` would also
/// work, this avoids the extra spin-loop dependency for a crate that
/// already ships `embassy-futures` for its own use).
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

#[derive(Default)]
struct FakeDriver {
    acquire_calls: u32,
    cancel_calls: u32,
    fail_next: bool,
    min_interval: Duration,
    last_requested: Option<TestFields>,
}

impl AcquisitionDriver<TestFields, i32> for FakeDriver {
    type Error = ();

    async fn acquire(&mut self, requested: TestFields) -> Result<(TestFields, i32), ()> {
        self.acquire_calls += 1;
        self.last_requested = Some(requested);
        if self.fail_next {
            self.fail_next = false;
            return Err(());
        }
        // A combined conversion always measures both fields, like SHTC3.
        let measured = TestFields::TEMPERATURE.union(TestFields::HUMIDITY);
        Ok((measured, 21))
    }

    async fn cancel(&mut self) -> Result<(), ()> {
        self.cancel_calls += 1;
        Ok(())
    }

    fn min_acquisition_interval(&self) -> Duration {
        self.min_interval
    }
}

/// A driver whose `acquire`/`cancel` advance a shared clock cell before
/// returning, standing in for real conversion/bus latency the fakes
/// above never model (`self.acquire_calls`'s siblings all resolve
/// "instantly" from the clock's point of view). A test holds the same
/// `&Cell` in its own `now` closure, so it can assert that `step`
/// re-reads the clock *after* the driver call rather than reusing the
/// `Instant` it read before awaiting it.
struct ClockAdvancingDriver<'a> {
    clock: &'a core::cell::Cell<u64>,
    advance_by_ms: u64,
    fail: bool,
    min_interval: Duration,
}

impl AcquisitionDriver<TestFields, i32> for ClockAdvancingDriver<'_> {
    type Error = ();

    async fn acquire(&mut self, requested: TestFields) -> Result<(TestFields, i32), ()> {
        self.clock.set(self.clock.get() + self.advance_by_ms);
        if self.fail {
            return Err(());
        }
        Ok((requested, 0))
    }

    async fn cancel(&mut self) -> Result<(), ()> {
        Ok(())
    }

    fn min_acquisition_interval(&self) -> Duration {
        self.min_interval
    }
}

struct FailingCancelDriver;
impl AcquisitionDriver<TestFields, i32> for FailingCancelDriver {
    type Error = ();
    async fn acquire(&mut self, requested: TestFields) -> Result<(TestFields, i32), ()> {
        Ok((requested, 0))
    }
    async fn cancel(&mut self) -> Result<(), ()> {
        Err(())
    }
    fn min_acquisition_interval(&self) -> Duration {
        Duration::ZERO
    }
}

fn one_shot(
    fields: TestFields,
    admitted_at: u64,
    expires_at: u64,
) -> ObserveNowRequest<TestFields> {
    ObserveNowRequest {
        owner: OwnerId(1),
        owner_generation: OwnerGeneration(0),
        fields,
        admitted_at: Instant(admitted_at),
        expires_at: Instant(expires_at),
    }
}

#[test]
fn idle_wait_when_demand_and_requests_are_empty() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    let demand = Demand::<TestFields, 4>::empty();

    let outcome = block_on(provider.step(&demand, || Instant::ZERO));
    assert_eq!(outcome, StepOutcome::Idle { next_wake: None });
    assert_eq!(provider.driver.acquire_calls, 0);
}

#[test]
fn due_demand_triggers_one_acquisition() {
    let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::TEMPERATURE,
        max_age: Duration::from_secs(60),
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::from_secs(60),
        expires_at: None,
    })
    .unwrap();
    let demand = aggregate_demand(&book, ProviderId(1), Instant::ZERO);

    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );

    let outcome = block_on(provider.step(&demand, || Instant::ZERO));
    assert_eq!(
        outcome,
        StepOutcome::Acquired {
            fields: TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            revision: Revision(1),
        }
    );
    assert_eq!(provider.state().health, Health::Ok);
    assert_eq!(provider.state().snapshot, 21);
}

#[test]
fn coalesces_a_pending_observe_now_with_due_periodic_demand() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::HUMIDITY, 0, 10_000))
        .unwrap();

    let demand = Demand::<TestFields, 4>::empty(); // no periodic demand at all
    let outcome = block_on(provider.step(&demand, || Instant::ZERO));

    assert!(matches!(outcome, StepOutcome::Acquired { .. }));
    assert_eq!(provider.driver.acquire_calls, 1);
    // The one-shot's field alone drove the single acquisition.
    assert_eq!(provider.driver.last_requested, Some(TestFields::HUMIDITY));
    assert_eq!(provider.pending_observe_now(), 0); // satisfied and removed
}

#[test]
fn expired_observe_now_request_is_purged_without_an_acquisition() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::HUMIDITY, 0, 100))
        .unwrap();

    let demand = Demand::<TestFields, 4>::empty();
    let outcome = block_on(provider.step(&demand, || Instant(200)));

    assert_eq!(outcome, StepOutcome::Idle { next_wake: None });
    assert_eq!(provider.driver.acquire_calls, 0);
    assert_eq!(provider.pending_observe_now(), 0);
}

#[test]
fn rate_limit_clamps_to_the_minimum_acquisition_interval() {
    let driver = FakeDriver {
        min_interval: Duration::from_secs(60),
        ..Default::default()
    };
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::TEMPERATURE,
        max_age: Duration::ZERO, // always due
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::ZERO,
        expires_at: None,
    })
    .unwrap();

    let demand = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
    block_on(provider.step(&demand, || Instant::ZERO));
    assert_eq!(provider.driver.acquire_calls, 1);

    // Immediately re-stepping is clamped by min_acquisition_interval,
    // even though the field is (by max_age) "due" again.
    let demand = aggregate_demand(&book, ProviderId(1), Instant(1_000));
    let outcome = block_on(provider.step(&demand, || Instant(1_000)));
    assert_eq!(
        outcome,
        StepOutcome::Idle {
            next_wake: Some(Instant(60_000))
        }
    );
    assert_eq!(provider.driver.acquire_calls, 1);
}

#[test]
fn failure_backs_off_then_recovers_on_the_next_success() {
    let driver = FakeDriver {
        fail_next: true,
        ..Default::default()
    };
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::TEMPERATURE,
        max_age: Duration::ZERO,
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::ZERO,
        expires_at: None,
    })
    .unwrap();

    let demand = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
    let failed = block_on(provider.step(&demand, || Instant::ZERO));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = failed else {
        panic!("expected AcquisitionFailed, got {failed:?}");
    };
    assert_eq!(provider.state().health, Health::Failed);
    assert!(retry_at > Instant::ZERO);

    // Retrying before the backoff elapses stays idle.
    let demand = aggregate_demand(&book, ProviderId(1), retry_at);
    assert_eq!(
        block_on(provider.step(&demand, || Instant(retry_at.0 - 1))),
        StepOutcome::Idle {
            next_wake: Some(retry_at)
        }
    );

    // At/after the backoff, it retries and recovers.
    let recovered = block_on(provider.step(&demand, || retry_at));
    assert!(matches!(recovered, StepOutcome::Acquired { .. }));
    assert_eq!(provider.state().health, Health::Ok);
}

/// E-0008: "Acquisition timestamps... reuse pre-await time." A driver
/// that models real conversion latency (advancing the shared clock
/// inside `acquire`) proves the success stamp lands at completion time,
/// not the `Instant` `step` read before awaiting the driver.
#[test]
fn success_is_stamped_at_completion_time_not_start_time() {
    let clock = core::cell::Cell::new(1_000u64);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 70,
        fail: false,
        min_interval: Duration::ZERO,
    };
    let mut provider: ProviderLoop<TestFields, i32, ClockAdvancingDriver, 4, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::TEMPERATURE,
        max_age: Duration::from_secs(60),
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::from_secs(60),
        expires_at: None,
    })
    .unwrap();

    let demand = aggregate_demand(&book, ProviderId(1), Instant(clock.get()));
    let now = || Instant(clock.get());
    let outcome = block_on(provider.step(&demand, now));
    assert!(matches!(outcome, StepOutcome::Acquired { .. }));
    // Started at 1000, the driver advanced the clock by 70 during
    // `acquire`: the stamp must be 1070, not the pre-await 1000.
    assert_eq!(
        provider.state().field_timestamps.get(0),
        Some(Instant(1_070))
    );
}

/// E-0008: "...and retry deadlines reuse pre-await time." A failing
/// driver that advances the clock before returning `Err` proves
/// `retry_at` is computed from completion time, not start time --
/// otherwise a conversion that itself takes longer than the resulting
/// backoff would produce a `retry_at` already in the past.
#[test]
fn failure_retry_deadline_is_based_on_completion_time_not_start_time() {
    let clock = core::cell::Cell::new(1_000u64);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 70,
        fail: true,
        min_interval: Duration::from_secs(60),
    };
    let mut provider: ProviderLoop<TestFields, i32, ClockAdvancingDriver, 4, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::TEMPERATURE,
        max_age: Duration::ZERO,
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::ZERO,
        expires_at: None,
    })
    .unwrap();

    let demand = aggregate_demand(&book, ProviderId(1), Instant(clock.get()));
    let now = || Instant(clock.get());
    let failed = block_on(provider.step(&demand, now));
    let StepOutcome::AcquisitionFailed { retry_at, .. } = failed else {
        panic!("expected AcquisitionFailed, got {failed:?}");
    };
    // Completion time is 1000 + 70 = 1070; the first failure's backoff
    // is `min_acquisition_interval` doubled once (`next_backoff` starts
    // from one minimum interval, then always doubles -- see its doc).
    assert_eq!(retry_at, Instant(1_070 + 120_000));
}

#[test]
fn suspension_closes_admission_and_discards_pending_one_shots() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 4, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 10_000))
        .unwrap();
    assert_eq!(provider.pending_observe_now(), 1);

    let outcome = block_on(provider.suspend(Instant::ZERO));
    assert_eq!(outcome, SuspendOutcome::Quiesced);
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(provider.driver.cancel_calls, 1);

    assert_eq!(
        provider.admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 10_000)),
        Err(QueueFull)
    );

    let demand = Demand::<TestFields, 4>::empty();
    assert_eq!(
        block_on(provider.step(&demand, || Instant::ZERO)),
        StepOutcome::Suspended
    );

    provider.resume();
    assert!(provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 10_000))
        .is_ok());
}

#[test]
fn suspension_is_available_even_with_a_full_request_queue() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 1, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 10_000))
        .unwrap();
    assert_eq!(
        provider.admit_observe_now(one_shot(TestFields::HUMIDITY, 0, 10_000)),
        Err(QueueFull)
    );

    assert_eq!(
        block_on(provider.suspend(Instant::ZERO)),
        SuspendOutcome::Quiesced
    );
}

#[test]
fn suspension_reports_driver_cleanup_failure() {
    let mut provider: ProviderLoop<TestFields, i32, FailingCancelDriver, 4, 4> = ProviderLoop::new(
        FailingCancelDriver,
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );

    assert_eq!(
        block_on(provider.suspend(Instant::ZERO)),
        SuspendOutcome::CleanupFailed
    );
}

#[test]
fn timed_admission_reuses_expired_slots_and_reports_rejections() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 1, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    let request = one_shot(TestFields::TEMPERATURE, 0, 100);
    assert_eq!(
        provider.admit_observe_now_at(request, Instant::ZERO),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(
        provider.admit_observe_now_at(request, Instant(1)),
        Err(RequestRejection::Full)
    );
    assert_eq!(
        provider.admit_observe_now_at(request, Instant(100)),
        Err(RequestRejection::Expired)
    );
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(
        provider.admit_observe_now_at(one_shot(TestFields::EMPTY, 100, 1_000), Instant(100)),
        Err(RequestRejection::Invalid)
    );
    assert_eq!(
        provider.admit_observe_now_at(one_shot(TestFields::TEMPERATURE, 101, 1_000), Instant(100)),
        Err(RequestRejection::Invalid)
    );
    assert_eq!(
        provider.admit_observe_now_at(one_shot(TestFields::HUMIDITY, 100, 1_000), Instant(100)),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(
        block_on(provider.suspend(Instant(100))),
        SuspendOutcome::Quiesced
    );
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(
        provider.admit_observe_now_at(one_shot(TestFields::HUMIDITY, 100, 1_000), Instant(100)),
        Err(RequestRejection::Suspended)
    );
}

struct PendingDriver<'a> {
    ready: &'a core::cell::Cell<bool>,
    calls: &'a core::cell::Cell<u32>,
}

impl AcquisitionDriver<TestFields, i32> for PendingDriver<'_> {
    type Error = ();

    async fn acquire(&mut self, requested: TestFields) -> Result<(TestFields, i32), ()> {
        self.calls.set(self.calls.get() + 1);
        core::future::poll_fn(|_| {
            if self.ready.get() {
                core::task::Poll::Ready(())
            } else {
                core::task::Poll::Pending
            }
        })
        .await;
        Ok((requested, 21))
    }

    async fn cancel(&mut self) -> Result<(), ()> {
        Ok(())
    }

    fn min_acquisition_interval(&self) -> Duration {
        Duration::ZERO
    }
}

#[test]
fn transport_arrivals_during_pending_conversion_coalesce_only_measured_fields() {
    use core::{
        cell::Cell,
        future::Future,
        task::{Context, Poll, Waker},
    };
    use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel};

    let clock = Cell::new(0);
    let ready = Cell::new(false);
    let calls = Cell::new(0);
    let transport: Channel<NoopRawMutex, ObserveNowRequest<TestFields>, 2> = Channel::new();
    let mut provider: ProviderLoop<TestFields, i32, PendingDriver<'_>, 2, 4> = ProviderLoop::new(
        PendingDriver {
            ready: &ready,
            calls: &calls,
        },
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 1_000))
        .unwrap();
    let demand = Demand::empty();
    let outcome = {
        let mut acquisition = core::pin::pin!(provider.step(&demand, || Instant(clock.get())));
        let mut context = Context::from_waker(Waker::noop());
        assert!(acquisition.as_mut().poll(&mut context).is_pending());
        clock.set(50);
        transport
            .try_send(one_shot(TestFields::TEMPERATURE, 50, 1_000))
            .unwrap();
        transport
            .try_send(one_shot(TestFields::HUMIDITY, 50, 1_000))
            .unwrap();
        clock.set(100);
        ready.set(true);
        let Poll::Ready(outcome) = acquisition.as_mut().poll(&mut context) else {
            panic!("ready driver must complete");
        };
        outcome
    };
    assert_eq!(provider.state().field_timestamps.get(0), Some(Instant(100)));
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(
        provider.admit_observe_now_after_step(
            transport.try_receive().unwrap(),
            outcome,
            Instant(100)
        ),
        Ok(RequestAdmission::Satisfied)
    );
    assert_eq!(
        provider.admit_observe_now_after_step(
            transport.try_receive().unwrap(),
            outcome,
            Instant(100)
        ),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(provider.pending_observe_now(), 1);
    assert_eq!(calls.get(), 1);
    assert_eq!(
        block_on(provider.step(&demand, || Instant(101))),
        StepOutcome::Acquired {
            fields: TestFields::HUMIDITY,
            revision: Revision(2)
        }
    );
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(calls.get(), 2);
    assert_eq!(
        block_on(provider.step(&demand, || Instant(102))),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn completion_expiry_discards_requests_but_keeps_successful_cache() {
    let clock = core::cell::Cell::new(0);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 100,
        fail: false,
        min_interval: Duration::ZERO,
    };
    let mut provider: ProviderLoop<TestFields, i32, _, 1, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let request = one_shot(TestFields::TEMPERATURE, 0, 100);
    provider.admit_observe_now(request).unwrap();
    let outcome = block_on(provider.step(&Demand::empty(), || Instant(clock.get())));
    assert!(matches!(outcome, StepOutcome::Acquired { .. }));
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(provider.state().field_timestamps.get(0), Some(Instant(100)));
    assert_eq!(
        provider.admit_observe_now_after_step(request, outcome, Instant(100)),
        Err(RequestRejection::Expired)
    );
    // Processing may cross the expiry tick even without another await.
    // A conversion completed in-window still satisfies its request.
    assert_eq!(
        provider.admit_observe_now_after_step(
            one_shot(TestFields::TEMPERATURE, 50, 101),
            outcome,
            Instant(101),
        ),
        Ok(RequestAdmission::Satisfied)
    );
    assert_eq!(provider.pending_observe_now(), 0);
}

#[test]
fn after_step_does_not_satisfy_later_admission_or_stale_outcome() {
    let mut provider: ProviderLoop<TestFields, i32, FakeDriver, 2, 4> = ProviderLoop::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 1_000))
        .unwrap();
    let demand = Demand::empty();
    let first = block_on(provider.step(&demand, || Instant(100)));
    assert_eq!(
        provider.admit_observe_now_after_step(
            one_shot(TestFields::TEMPERATURE, 101, 1_000),
            first,
            Instant(101)
        ),
        Ok(RequestAdmission::Queued)
    );
    block_on(provider.step(&demand, || Instant(110)));
    assert_eq!(
        provider.admit_observe_now_after_step(
            one_shot(TestFields::TEMPERATURE, 105, 1_000),
            first,
            Instant(110)
        ),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(provider.pending_observe_now(), 1);
}

#[test]
fn failed_conversion_does_not_satisfy_new_request_from_existing_cache() {
    let clock = core::cell::Cell::new(0);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 100,
        fail: false,
        min_interval: Duration::ZERO,
    };
    let mut provider: ProviderLoop<TestFields, i32, _, 1, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    let request = one_shot(TestFields::TEMPERATURE, 0, 1_000);
    provider.admit_observe_now(request).unwrap();
    let demand = Demand::empty();
    block_on(provider.step(&demand, || Instant(clock.get())));
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 100, 150))
        .unwrap();
    provider.driver.fail = true;
    let failed = block_on(provider.step(&demand, || Instant(clock.get())));
    assert!(matches!(failed, StepOutcome::AcquisitionFailed { .. }));
    assert_eq!(provider.pending_observe_now(), 0); // expired during failed conversion
    assert_eq!(
        provider.admit_observe_now_after_step(
            one_shot(TestFields::TEMPERATURE, 150, 1_000),
            failed,
            Instant(200)
        ),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(provider.state().field_timestamps.get(0), Some(Instant(100)));
}

#[test]
fn early_demand_and_request_wakes_preserve_completion_based_backoff() {
    let clock = core::cell::Cell::new(0);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 100,
        fail: true,
        min_interval: Duration(10),
    };
    let mut provider: ProviderLoop<TestFields, i32, _, 2, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    provider
        .admit_observe_now(one_shot(TestFields::TEMPERATURE, 0, 1_000))
        .unwrap();
    let failed = block_on(provider.step(&Demand::empty(), || Instant(clock.get())));
    assert_eq!(
        failed,
        StepOutcome::AcquisitionFailed {
            revision: Revision(1),
            retry_at: Instant(120)
        }
    );
    let mut book = SubscriptionBook::<TestFields, 1>::new();
    book.upsert(ObservationSubscription {
        key: SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(2),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: TestFields::HUMIDITY,
        max_age: Duration::ZERO,
        initial_policy: InitialPolicy::UseCache,
        min_delivery_interval: Duration::ZERO,
        expires_at: None,
    })
    .unwrap();
    let demand = aggregate_demand(&book, ProviderId(1), Instant(101));
    clock.set(101);
    provider
        .admit_observe_now_at(one_shot(TestFields::HUMIDITY, 101, 1_000), Instant(101))
        .unwrap();
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Idle {
            next_wake: Some(Instant(120))
        }
    );
    assert_eq!(clock.get(), 101); // driver did not run again
    assert_eq!(provider.state().revision, Revision(1));
    assert_eq!(
        block_on(provider.suspend(Instant(101))),
        SuspendOutcome::Quiesced
    );
    provider.resume();
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Idle {
            next_wake: Some(Instant(120))
        }
    );
    clock.set(120);
    provider.driver.fail = false;
    assert!(matches!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Acquired { .. }
    ));
    assert_eq!(clock.get(), 220);
}

#[test]
fn request_expiry_wakes_before_backoff_to_release_capacity_without_acquiring() {
    let clock = core::cell::Cell::new(0);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 100,
        fail: true,
        min_interval: Duration(1_000),
    };
    let mut provider: ProviderLoop<TestFields, i32, _, 1, 4> =
        ProviderLoop::new(driver, ProviderId(1), ProviderGeneration::INITIAL, 0);
    provider
        .admit_observe_now_at(one_shot(TestFields::TEMPERATURE, 0, 500), Instant::ZERO)
        .unwrap();
    let demand = Demand::empty();
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::AcquisitionFailed {
            revision: Revision(1),
            retry_at: Instant(2_100),
        }
    );
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Idle {
            next_wake: Some(Instant(500)),
        }
    );
    assert_eq!(provider.pending_observe_now(), 1);
    clock.set(500);
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Idle { next_wake: None }
    );
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(clock.get(), 500); // expiry did not poll the driver
    assert_eq!(provider.state().revision, Revision(1));
    assert_eq!(
        provider.admit_observe_now_at(one_shot(TestFields::HUMIDITY, 500, 3_000), Instant(500),),
        Ok(RequestAdmission::Queued)
    );
    assert_eq!(
        block_on(provider.step(&demand, || Instant(clock.get()))),
        StepOutcome::Idle {
            next_wake: Some(Instant(2_100)),
        }
    );
}

#[test]
fn periodic_override_restores_live_acquisition_with_ui_absent_and_backoff_preserved() {
    use crate::periodic::{DemandControl, PeriodicRequest, PeriodicStatus};
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    let control = DemandControl::<NoopRawMutex, TestFields, 2>::new();
    control.publish(Demand::uniform(TestFields::TEMPERATURE, Duration(300_000)));
    let mut provider = ProviderLoop::<TestFields, i32, _, 2, 2>::new(
        FakeDriver::default(),
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    control
        .begin(
            PeriodicRequest {
                id: 1,
                interval: Duration(60_000),
                expires_at: Instant(150_000),
            },
            TestFields::HUMIDITY,
            Instant(0),
            0,
        )
        .unwrap();
    for at in [0, 60_000, 120_000] {
        let selected = control.resolve(Instant(at), 0);
        assert!(matches!(
            block_on(provider.step(&selected.demand, || Instant(at))),
            StepOutcome::Acquired { .. }
        ));
    }
    // No UI callback to withdraw/restore: the same provider selects live demand.
    let selected = control.resolve(Instant(150_000), 0);
    assert_eq!(selected.event.unwrap().status, PeriodicStatus::Restored);
    assert_eq!(
        block_on(provider.step(&selected.demand, || Instant(150_000))),
        StepOutcome::Idle {
            next_wake: Some(Instant(420_000))
        }
    );
    assert_eq!(provider.driver.acquire_calls, 3);
    assert!(matches!(
        block_on(provider.step(&selected.demand, || Instant(420_000))),
        StepOutcome::Acquired { .. }
    ));
}

#[test]
fn override_waits_for_in_flight_acquisition_and_expiry_does_not_reset_backoff() {
    use crate::periodic::{DemandControl, PeriodicRequest, PeriodicStatus};
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    let control = DemandControl::<NoopRawMutex, TestFields, 2>::new();
    control.publish(Demand::uniform(TestFields::TEMPERATURE, Duration(300_000)));
    let clock = core::cell::Cell::new(0);
    let driver = ClockAdvancingDriver {
        clock: &clock,
        advance_by_ms: 200,
        fail: true,
        min_interval: Duration(100_000),
    };
    let mut provider = ProviderLoop::<TestFields, i32, _, 2, 2>::new(
        driver,
        ProviderId(1),
        ProviderGeneration::INITIAL,
        0,
    );
    let live = control.resolve(Instant(0), 0);
    control
        .begin(
            PeriodicRequest {
                id: 1,
                interval: Duration(60_000),
                expires_at: Instant(150_000),
            },
            TestFields::HUMIDITY,
            Instant(0),
            0,
        )
        .unwrap();
    // The already-selected acquisition still uses live fields and settles first.
    assert!(matches!(
        block_on(provider.step(&live.demand, || Instant(clock.get()))),
        StepOutcome::AcquisitionFailed { .. }
    ));
    let selected = control.resolve(Instant(clock.get()), 0);
    assert_eq!(selected.event.unwrap().applied_at, Some(Instant(200)));
    assert_eq!(
        selected.deadline(Some(Instant(200_200))),
        Some(Instant(150_000))
    );
    let restored = control.resolve(Instant(150_000), 0);
    assert_eq!(restored.event.unwrap().status, PeriodicStatus::Restored);
    assert_eq!(
        block_on(provider.step(&restored.demand, || Instant(150_000))),
        StepOutcome::Idle {
            next_wake: Some(Instant(200_200))
        }
    );
}
