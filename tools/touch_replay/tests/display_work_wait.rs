#[path = "../../../products/meditamer/src/firmware/display/work_wait.rs"]
mod work_wait;
use core::{
    future::{pending, ready, Future},
    pin::Pin,
    task::{Context, Poll, Waker},
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
type Queue = Channel<CriticalSectionRawMutex, u8, 2>;
struct Count(AtomicUsize);
impl std::task::Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
fn poll<F: Future>(f: Pin<&mut F>, w: &Waker) -> Poll<F::Output> {
    f.poll(&mut Context::from_waker(w))
}

#[test]
fn all_work_sources_wake_without_consuming_their_queue() {
    for source in 0..4 {
        let (app, sd, touch, multi) = (Queue::new(), Queue::new(), Queue::new(), Queue::new());
        let signal = Signal::new();
        let count = Arc::new(Count(AtomicUsize::new(0)));
        let waker = Waker::from(count.clone());
        let mut f = core::pin::pin!(work_wait::wait(
            &app,
            &sd,
            &touch,
            &multi,
            true,
            &signal,
            pending()
        ));
        assert!(poll(f.as_mut(), &waker).is_pending());
        let queue = [&app, &sd, &touch, &multi][source];
        queue.try_send(42).unwrap();
        assert!(count.0.load(Ordering::Relaxed) > 0);
        assert_eq!(
            poll(f.as_mut(), &waker),
            Poll::Ready(if source == 0 { Some(42) } else { None })
        );
        if source != 0 {
            assert_eq!(queue.try_receive(), Ok(42));
        }
    }
}
#[test]
fn simultaneous_app_and_touch_preserve_touch_for_ordered_merge() {
    let (a, s, t, m) = (Queue::new(), Queue::new(), Queue::new(), Queue::new());
    let w = Signal::new();
    a.try_send(1).unwrap();
    t.try_send(2).unwrap();
    m.try_send(3).unwrap();
    let mut f = core::pin::pin!(work_wait::wait(&a, &s, &t, &m, true, &w, pending()));
    assert_eq!(poll(f.as_mut(), Waker::noop()), Poll::Ready(Some(1)));
    assert_eq!(t.try_receive(), Ok(2));
    assert_eq!(m.try_receive(), Ok(3));
}
#[test]
fn notification_is_latched_before_wait_and_deadline_needs_no_event() {
    let (a, s, t, m) = (Queue::new(), Queue::new(), Queue::new(), Queue::new());
    let w = Signal::new();
    w.signal(());
    let mut f = core::pin::pin!(work_wait::wait(&a, &s, &t, &m, true, &w, pending()));
    assert_eq!(poll(f.as_mut(), Waker::noop()), Poll::Ready(None));
    let mut f = core::pin::pin!(work_wait::wait(&a, &s, &t, &m, true, &w, ready(())));
    assert_eq!(poll(f.as_mut(), Waker::noop()), Poll::Ready(None));
}
#[test]
fn recovery_does_not_spin_on_touch_but_sd_remains_live() {
    let (a, s, t, m) = (Queue::new(), Queue::new(), Queue::new(), Queue::new());
    let w = Signal::new();
    t.try_send(1).unwrap();
    let mut f = core::pin::pin!(work_wait::wait(&a, &s, &t, &m, false, &w, pending()));
    assert!(poll(f.as_mut(), Waker::noop()).is_pending());
    s.try_send(2).unwrap();
    assert_eq!(poll(f.as_mut(), Waker::noop()), Poll::Ready(None));
    assert_eq!(t.try_receive(), Ok(1));
    assert_eq!(s.try_receive(), Ok(2));
}
