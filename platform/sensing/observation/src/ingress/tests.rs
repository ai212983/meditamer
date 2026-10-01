use super::*;
use crate::{
    demand::Demand,
    ids::{OwnerGeneration, OwnerId, ProviderGeneration, ProviderId},
    runtime::{AcquisitionDriver, ProviderLoop, RequestAdmission, RequestRejection},
    time::Duration,
};
use core::{cell::Cell, future::Future, task::Context, task::Poll, task::Waker};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fields(u32);

impl FieldMask for Fields {
    const EMPTY: Self = Self(0);

    fn bits(self) -> u32 {
        self.0
    }

    fn from_bits_truncate(bits: u32) -> Self {
        Self(bits & 1)
    }
}

type Ingress<const N: usize> = RequestIngress<NoopRawMutex, Fields, N>;

fn request(owner: u16) -> ObserveNowRequest<Fields> {
    ObserveNowRequest {
        owner: OwnerId(owner),
        owner_generation: OwnerGeneration(0),
        fields: Fields(1),
        admitted_at: Instant(0),
        expires_at: Instant(100),
    }
}

struct Driver<'a> {
    ready: &'a Cell<bool>,
    acquisitions: &'a Cell<u32>,
    cancellations: &'a Cell<u32>,
}

impl AcquisitionDriver<Fields, u8> for Driver<'_> {
    type Error = ();

    async fn acquire(&mut self, requested: Fields) -> Result<(Fields, u8), ()> {
        self.acquisitions.set(self.acquisitions.get() + 1);
        poll_fn(|_| {
            if self.ready.get() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        Ok((requested, 42))
    }

    async fn cancel(&mut self) -> Result<(), ()> {
        self.cancellations.set(self.cancellations.get() + 1);
        Ok(())
    }

    fn min_acquisition_interval(&self) -> Duration {
        Duration::ZERO
    }
}

fn immediately<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("expected immediate completion"),
    }
}

#[test]
fn full_includes_dequeued_provider_pending_requests() {
    let ingress = Ingress::<2>::new();
    ingress.try_admit(request(1), Instant(1)).unwrap();
    ingress.try_admit(request(2), Instant(2)).unwrap();
    assert_eq!(ingress.outstanding(), 2);
    assert_eq!(
        ingress.try_admit(request(3), Instant(3)),
        Err(RequestAdmissionError::Full)
    );
    assert_eq!(ingress.try_receive().unwrap().owner, OwnerId(1));
    assert_eq!(ingress.try_receive().unwrap().owner, OwnerId(2));
    assert!(ingress.try_receive().is_err());
    assert_eq!(ingress.outstanding(), 2);
    assert_eq!(
        ingress.try_admit(request(3), Instant(3)),
        Err(RequestAdmissionError::Full)
    );
}

#[test]
fn expiry_rejects_without_spending_credit() {
    let ingress = Ingress::<1>::new();
    for now in [100, 101] {
        assert_eq!(
            ingress.try_admit(request(1), Instant(now)),
            Err(RequestAdmissionError::Expired)
        );
    }
    assert_eq!(ingress.outstanding(), 0);
    assert!(ingress.try_receive().is_err());
    ingress.try_admit(request(1), Instant(99)).unwrap();
    assert_eq!(ingress.try_receive().unwrap().admitted_at, Instant(0));
}

#[test]
fn close_reclaims_only_queued_credits_and_is_idempotent() {
    let ingress = Ingress::<2>::new();
    ingress.try_admit(request(1), Instant(1)).unwrap();
    ingress.try_admit(request(2), Instant(2)).unwrap();
    ingress.try_receive().unwrap();
    assert_eq!(ingress.close_generation(), 0);
    assert_eq!(ingress.close(), 1);
    assert_eq!(ingress.close_generation(), 1);
    assert_eq!(ingress.outstanding(), 1);
    assert!(ingress.try_receive().is_err());
    assert_eq!(ingress.close(), 0);
    assert_eq!(ingress.close_generation(), 1);
    assert_eq!(
        ingress.try_admit(request(3), Instant(3)),
        Err(RequestAdmissionError::Closed)
    );
    ingress.complete(1);
    assert_eq!(ingress.outstanding(), 0);
    assert!(ingress.open());
    ingress.try_admit(request(3), Instant(3)).unwrap();
    assert_eq!(ingress.outstanding(), 1);
}

#[test]
fn reopening_does_not_discard_pending_credit() {
    let ingress = Ingress::<1>::new();
    ingress.try_admit(request(1), Instant(1)).unwrap();
    ingress.try_receive().unwrap();
    ingress.close();
    ingress.open();
    assert_eq!(
        ingress.try_admit(request(2), Instant(2)),
        Err(RequestAdmissionError::Full)
    );
    ingress.complete(1);
    ingress.try_admit(request(2), Instant(2)).unwrap();
}

#[test]
fn credits_recycle_after_completion_with_another_request_still_queued() {
    let ingress = Ingress::<2>::new();
    for owner in 1..=2 {
        ingress.try_admit(request(owner), Instant(1)).unwrap();
    }
    ingress.try_receive().unwrap();
    ingress.complete(1);
    ingress.try_admit(request(3), Instant(3)).unwrap();
    assert_eq!(ingress.outstanding(), 2);
    assert_eq!(ingress.try_receive().unwrap().owner, OwnerId(2));
    assert_eq!(ingress.try_receive().unwrap().owner, OwnerId(3));
    ingress.complete(2);
    ingress.complete(0);
    assert_eq!(ingress.outstanding(), 0);
}

#[test]
fn pending_receive_can_be_cancelled_and_later_receive_keeps_credit() {
    let ingress = Ingress::<1>::new();
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut receive = core::pin::pin!(ingress.receive());
        assert!(receive.as_mut().poll(&mut cx).is_pending());
    }
    ingress.try_admit(request(1), Instant(1)).unwrap();
    let mut receive = core::pin::pin!(ingress.receive());
    assert_eq!(receive.as_mut().poll(&mut cx), Poll::Ready(request(1)));
    assert_eq!(ingress.outstanding(), 1);
    ingress.complete(1);
    assert_eq!(ingress.outstanding(), 0);
}

#[test]
fn invalid_requests_do_not_spend_credits() {
    let ingress = Ingress::<1>::new();
    let mut invalid = request(1);
    invalid.fields = Fields::EMPTY;
    assert_eq!(
        ingress.try_admit(invalid, Instant(1)),
        Err(RequestAdmissionError::Invalid)
    );
    invalid = request(1);
    invalid.admitted_at = Instant(2);
    assert_eq!(
        ingress.try_admit(invalid, Instant(1)),
        Err(RequestAdmissionError::Invalid)
    );
    assert_eq!(ingress.outstanding(), 0);
}

#[test]
fn generation_exhaustion_stays_closed_and_still_reclaims_queue() {
    let ingress = Ingress::<1>::new();
    ingress
        .credits
        .lock(|state| state.borrow_mut().close_generation = u32::MAX - 1);
    ingress.try_admit(request(1), Instant(1)).unwrap();
    assert_eq!(ingress.close(), 1);
    assert_eq!(ingress.close_generation(), u32::MAX);
    assert_eq!(ingress.outstanding(), 0);
    assert!(!ingress.open());
    assert_eq!(ingress.close(), 0);
    assert_eq!(ingress.close_generation(), u32::MAX);
    assert_eq!(
        ingress.try_admit(request(2), Instant(2)),
        Err(RequestAdmissionError::Closed)
    );
}

#[test]
fn readiness_retains_queue_for_control_to_close() {
    let ingress = Ingress::<1>::new();
    let mut cx = Context::from_waker(Waker::noop());
    let mut ready = core::pin::pin!(ingress.ready_to_receive());
    assert!(ready.as_mut().poll(&mut cx).is_pending());
    ingress.try_admit(request(1), Instant(1)).unwrap();
    assert!(ready.as_mut().poll(&mut cx).is_ready());
    assert_eq!(ingress.outstanding(), 1);
    assert_eq!(ingress.close(), 1);
    assert_eq!(ingress.outstanding(), 0);
    assert!(ingress.try_receive().is_err());
}

#[test]
#[should_panic(expected = "observation request credit underflow")]
fn double_completion_is_not_silently_ignored() {
    let ingress = Ingress::<1>::new();
    ingress.try_admit(request(1), Instant(1)).unwrap();
    ingress.try_receive().unwrap();
    ingress.complete(1);
    ingress.complete(1);
}

#[test]
#[should_panic(expected = "cannot complete queued observation requests")]
fn consumer_cannot_complete_still_queued_request() {
    let ingress = Ingress::<1>::new();
    ingress.try_admit(request(1), Instant(1)).unwrap();
    ingress.complete(1);
}

#[test]
fn requests_admitted_during_conversion_share_credit_bound_and_result() {
    let ingress = Ingress::<2>::new();
    let ready = Cell::new(false);
    let acquisitions = Cell::new(0);
    let cancellations = Cell::new(0);
    let mut provider = ProviderLoop::<_, _, _, 2, 1>::new(
        Driver {
            ready: &ready,
            acquisitions: &acquisitions,
            cancellations: &cancellations,
        },
        ProviderId(1),
        ProviderGeneration(0),
        0,
    );
    ingress.try_admit(request(1), Instant(1)).unwrap();
    assert_eq!(
        provider.admit_observe_now_at(ingress.try_receive().unwrap(), Instant(1)),
        Ok(RequestAdmission::Queued)
    );
    let before = provider.pending_observe_now();
    let clock = Cell::new(5);
    let demand = Demand::<Fields, 1>::empty();
    let outcome = {
        let mut step = core::pin::pin!(provider.step(&demand, || Instant(clock.get())));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(step.as_mut().poll(&mut cx).is_pending());
        let mut during = request(2);
        during.admitted_at = Instant(6);
        ingress.try_admit(during, Instant(6)).unwrap();
        assert_eq!(ingress.outstanding(), before + ingress.channel.len());
        assert_eq!(ingress.outstanding(), 2);
        assert_eq!(
            ingress.try_admit(request(3), Instant(6)),
            Err(RequestAdmissionError::Full)
        );
        clock.set(10);
        ready.set(true);
        match step.as_mut().poll(&mut cx) {
            Poll::Ready(outcome) => outcome,
            Poll::Pending => panic!("conversion remained pending"),
        }
    };
    ingress.complete(before - provider.pending_observe_now());
    let before = provider.pending_observe_now();
    assert_eq!(
        provider
            .admit_observe_now_after_step(ingress.try_receive().unwrap(), outcome, Instant(10),),
        Ok(RequestAdmission::Satisfied)
    );
    ingress.complete(before + 1 - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(provider.pending_observe_now(), 0);
    assert_eq!(provider.state().snapshot, 42);
    immediately(provider.step(&demand, || Instant(11)));
    assert_eq!(acquisitions.get(), 1);
}

#[test]
fn rejection_releases_current_credit_and_expired_provider_credits() {
    let ingress = Ingress::<2>::new();
    let ready = Cell::new(true);
    let acquisitions = Cell::new(0);
    let cancellations = Cell::new(0);
    let mut provider = ProviderLoop::<_, _, _, 2, 1>::new(
        Driver {
            ready: &ready,
            acquisitions: &acquisitions,
            cancellations: &cancellations,
        },
        ProviderId(1),
        ProviderGeneration(0),
        0,
    );
    ingress.try_admit(request(1), Instant(1)).unwrap();
    provider
        .admit_observe_now_at(ingress.try_receive().unwrap(), Instant(1))
        .unwrap();
    ingress.try_admit(request(2), Instant(2)).unwrap();
    assert_eq!(ingress.outstanding(), 2);
    let before = provider.pending_observe_now();
    assert_eq!(
        provider.admit_observe_now_at(ingress.try_receive().unwrap(), Instant(100)),
        Err(RequestRejection::Expired)
    );
    let released = before + 1 - provider.pending_observe_now();
    assert_eq!(released, 2);
    ingress.complete(released);
    assert_eq!(ingress.outstanding(), 0);
    let mut next = request(3);
    next.expires_at = Instant(200);
    ingress.try_admit(next, Instant(100)).unwrap();
    assert_eq!(acquisitions.get(), 0);
}
