#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
#[path = "../../../../products/meditamer/src/firmware/storage/upload/http/accept_wait.rs"]
mod accept_wait;

#[cfg(test)]
mod tests {
    use super::accept_wait::wait_accept;
    use core::cell::Cell;
    use core::future::Future;
    use core::pin::Pin;
    use core::task::{Context, Poll};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum AcceptError {
        Refused,
        Restarted,
    }

    struct Listener {
        listen_calls: Cell<usize>,
        accept_polls: Cell<usize>,
        poisoned: Cell<bool>,
        completed_drops: Cell<usize>,
        cancelled_drops: Cell<usize>,
    }

    impl Listener {
        fn new() -> Self {
            Self {
                listen_calls: Cell::new(0),
                accept_polls: Cell::new(0),
                poisoned: Cell::new(false),
                completed_drops: Cell::new(0),
                cancelled_drops: Cell::new(0),
            }
        }

        fn listen(&self, polls_needed: usize, outcome: Result<(), AcceptError>) -> FakeAccept<'_> {
            self.listen_calls.set(self.listen_calls.get() + 1);
            FakeAccept {
                listener: self,
                polls_needed,
                polls_done: 0,
                outcome: Some(outcome),
                settled: false,
                announced: false,
            }
        }
    }

    struct FakeAccept<'a> {
        listener: &'a Listener,
        polls_needed: usize,
        polls_done: usize,
        outcome: Option<Result<(), AcceptError>>,
        settled: bool,
        announced: bool,
    }

    impl Future for FakeAccept<'_> {
        type Output = Result<(), AcceptError>;

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            if this.listener.poisoned.get() {
                this.settled = true;
                return Poll::Ready(Err(AcceptError::Restarted));
            }
            if !this.announced {
                this.announced = true;
            }
            this.listener
                .accept_polls
                .set(this.listener.accept_polls.get() + 1);
            this.polls_done += 1;
            if this.polls_done >= this.polls_needed {
                this.settled = true;
                Poll::Ready(this.outcome.take().expect("polled after completion"))
            } else {
                Poll::Pending
            }
        }
    }

    impl Drop for FakeAccept<'_> {
        fn drop(&mut self) {
            if self.settled {
                self.listener
                    .completed_drops
                    .set(self.listener.completed_drops.get() + 1);
            } else {
                self.listener
                    .cancelled_drops
                    .set(self.listener.cancelled_drops.get() + 1);
                if self.announced {
                    self.listener.poisoned.set(true);
                }
            }
        }
    }

    // A dropped handshake that already saw its SYN poisons any later listen:
    // re-arming accept on real embassy-net/smoltcp stacks rejects the
    // in-progress handshake, so a helper that restarts accept across ticks
    // observes Restarted instead of the pending connection.
    #[test]
    fn handshake_completes_after_multiple_gate_ticks_without_relisten() {
        let listener = Listener::new();
        let accept = listener.listen(3, Ok(()));
        let ticks = Cell::new(0usize);
        let outcome = embassy_futures::block_on(wait_accept(
            accept,
            || true,
            || {
                ticks.set(ticks.get() + 1);
                core::future::ready(())
            },
        ));
        assert_eq!(outcome, Some(Ok(())));
        assert_eq!(listener.listen_calls.get(), 1);
        assert_eq!(listener.accept_polls.get(), 3);
        assert!(ticks.get() >= 2);
        assert!(!listener.poisoned.get());
        assert_eq!(listener.completed_drops.get(), 1);
        assert_eq!(listener.cancelled_drops.get(), 0);
    }

    #[test]
    fn gate_closure_during_handshake_cancels_once_with_none() {
        let listener = Listener::new();
        let accept = listener.listen(10, Ok(()));
        let gate_calls = Cell::new(0usize);
        let outcome = embassy_futures::block_on(wait_accept(
            accept,
            || {
                gate_calls.set(gate_calls.get() + 1);
                gate_calls.get() <= 2
            },
            || core::future::ready(()),
        ));
        assert!(outcome.is_none());
        assert_eq!(listener.listen_calls.get(), 1);
        assert_eq!(listener.accept_polls.get(), 2);
        assert_eq!(listener.completed_drops.get(), 0);
        assert_eq!(listener.cancelled_drops.get(), 1);
    }

    #[test]
    fn initially_closed_gate_never_polls_accept() {
        let listener = Listener::new();
        let accept = listener.listen(1, Ok(()));
        let outcome =
            embassy_futures::block_on(wait_accept(accept, || false, || core::future::ready(())));
        assert!(outcome.is_none());
        assert_eq!(listener.listen_calls.get(), 1);
        assert_eq!(listener.accept_polls.get(), 0);
        assert_eq!(listener.completed_drops.get(), 0);
        assert_eq!(listener.cancelled_drops.get(), 1);
    }

    #[test]
    fn accept_error_propagates_unchanged() {
        let listener = Listener::new();
        let accept = listener.listen(1, Err(AcceptError::Refused));
        let outcome =
            embassy_futures::block_on(wait_accept(accept, || true, || core::future::ready(())));
        assert_eq!(outcome, Some(Err(AcceptError::Refused)));
        assert_eq!(listener.listen_calls.get(), 1);
        assert_eq!(listener.completed_drops.get(), 1);
        assert_eq!(listener.cancelled_drops.get(), 0);
    }
}
