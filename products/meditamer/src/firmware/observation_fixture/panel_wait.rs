//! One UI waiter observes retained provider application state. Notifications
//! are hints only, so a stale/coalesced wake cannot acknowledge another ID.
use core::future::Future;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::RawMutex, signal::Signal};
use observation::{field::FieldMask, fixture::FixtureStatus, periodic::DemandControl};

pub(crate) async fn wait_for_application<M: RawMutex, F: FieldMask, const N: usize>(
    control: &DemandControl<M, F, N>,
    changed: &Signal<M, ()>,
    id: u64,
    deadline: impl Future<Output = ()>,
) -> Result<(), FixtureStatus> {
    let mut deadline = core::pin::pin!(deadline);
    loop {
        match control.application(id) {
            Some(true) => return Ok(()),
            None => return Err(FixtureStatus::Closed),
            Some(false) => {}
        }
        if let Either::Second(()) = select(changed.wait(), deadline.as_mut()).await {
            return Err(FixtureStatus::Expired);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::BatteryFields;
    use super::*;
    use core::{
        future::{pending, ready},
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use observation::{
        periodic::PeriodicRequest,
        time::{Duration, Instant},
    };

    type Control = DemandControl<NoopRawMutex, BatteryFields, 1>;
    fn pending_control() -> Control {
        let control = Control::new();
        control
            .begin(
                PeriodicRequest {
                    id: 1,
                    interval: Duration(60000),
                    expires_at: Instant(150000),
                },
                BatteryFields::LEVEL,
                Instant(0),
                0,
            )
            .unwrap();
        control
    }
    fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn application_requires_provider_resolution_despite_stale_notifications() {
        let control = pending_control();
        let changed = Signal::new();
        changed.signal(());
        let mut wait = core::pin::pin!(wait_for_application(&control, &changed, 1, pending()));
        assert_eq!(poll(wait.as_mut()), Poll::Pending);
        control.resolve(Instant(10), 0);
        changed.signal(());
        assert_eq!(poll(wait.as_mut()), Poll::Ready(Ok(())));
    }

    #[test]
    fn retained_application_is_visible_before_wait_and_wrong_id_cannot_pass() {
        let control = pending_control();
        let changed = Signal::new();
        control.resolve(Instant(10), 0);
        let mut wait = core::pin::pin!(wait_for_application(&control, &changed, 1, pending()));
        assert_eq!(poll(wait.as_mut()), Poll::Ready(Ok(())));
        let mut wrong = core::pin::pin!(wait_for_application(&control, &changed, 2, pending()));
        assert_eq!(
            poll(wrong.as_mut()),
            Poll::Ready(Err(FixtureStatus::Closed))
        );
    }

    #[test]
    fn closure_and_deadline_release_the_ui_without_applying_a_request() {
        let control = pending_control();
        let changed = Signal::new();
        let mut expired = core::pin::pin!(wait_for_application(&control, &changed, 1, ready(())));
        assert_eq!(
            poll(expired.as_mut()),
            Poll::Ready(Err(FixtureStatus::Expired))
        );
        assert_eq!(control.application(1), Some(false));
        let mut wait = core::pin::pin!(wait_for_application(&control, &changed, 1, pending()));
        assert_eq!(poll(wait.as_mut()), Poll::Pending);
        control.end(Instant(5), false);
        changed.signal(());
        assert_eq!(poll(wait.as_mut()), Poll::Ready(Err(FixtureStatus::Closed)));
    }
}
