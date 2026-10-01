//! Bounded request admission across the channel and the provider's pending queue.
//!
//! Receiving transfers a request, not its credit. The consumer returns credits
//! only after the provider removes the corresponding requests. Sleep control
//! travels separately so full admission cannot delay suspension.

use core::cell::RefCell;
use core::future::poll_fn;

use crate::{field::FieldMask, observe_now::ObserveNowRequest, time::Instant};
use embassy_sync::blocking_mutex::{raw::RawMutex, Mutex};
use embassy_sync::channel::{Channel, TryReceiveError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestAdmissionError {
    Full,
    Closed,
    Expired,
    Invalid,
}

struct Credits {
    open: bool,
    outstanding: usize,
    close_generation: u32,
}

pub struct RequestIngress<M: RawMutex, F: FieldMask, const N: usize> {
    channel: Channel<M, ObserveNowRequest<F>, N>,
    credits: Mutex<M, RefCell<Credits>>,
}

impl<M: RawMutex, F: FieldMask, const N: usize> RequestIngress<M, F, N> {
    pub const fn new() -> Self {
        Self {
            channel: Channel::new(),
            credits: Mutex::new(RefCell::new(Credits {
                open: true,
                outstanding: 0,
                close_generation: 0,
            })),
        }
    }

    /// Success means admitted, including capacity in the provider's pending
    /// queue. The caller supplies the admission time used by conversion cutoffs.
    pub fn try_admit(
        &self,
        request: ObserveNowRequest<F>,
        now: Instant,
    ) -> Result<(), RequestAdmissionError> {
        self.credits.lock(|state| {
            let mut state = state.borrow_mut();
            if !state.open {
                return Err(RequestAdmissionError::Closed);
            }
            if request.expires_at <= now {
                return Err(RequestAdmissionError::Expired);
            }
            if request.fields.is_empty() || request.admitted_at > now {
                return Err(RequestAdmissionError::Invalid);
            }
            if state.outstanding == N {
                return Err(RequestAdmissionError::Full);
            }
            self.channel
                .try_send(request)
                .map_err(|_| RequestAdmissionError::Full)?;
            state.outstanding += 1;
            Ok(())
        })
    }

    /// Transfer to the consumer while retaining its outstanding credit.
    pub fn try_receive(&self) -> Result<ObserveNowRequest<F>, TryReceiveError> {
        self.credits.lock(|_| self.channel.try_receive())
    }

    /// Cancellation while pending does not receive a request or spend a credit.
    /// Every poll locks credits before the channel, matching admission/closure;
    /// no lock is held across an await.
    #[cfg(test)]
    pub async fn receive(&self) -> ObserveNowRequest<F> {
        poll_fn(|cx| self.credits.lock(|_| self.channel.poll_receive(cx))).await
    }

    /// Wake the consumer without transferring a credit. It can check control
    /// first and then drain via `try_receive`, so suspension never abandons a
    /// request that a competing select branch already dequeued.
    pub async fn ready_to_receive(&self) {
        poll_fn(|cx| {
            self.credits
                .lock(|_| self.channel.poll_ready_to_receive(cx))
        })
        .await
    }

    /// Return credits for requests removed by the provider (including rejected
    /// or expired requests). Completing a still-queued request is a caller bug.
    pub fn complete(&self, count: usize) {
        self.credits.lock(|state| {
            let mut state = state.borrow_mut();
            let remaining = state
                .outstanding
                .checked_sub(count)
                .expect("observation request credit underflow");
            assert!(
                remaining >= self.channel.len(),
                "cannot complete queued observation requests"
            );
            state.outstanding = remaining;
        });
    }

    /// Atomically stop admission and discard queued requests, reclaiming their
    /// credits. Already-received requests remain the consumer's responsibility;
    /// it must remove them and call `complete` before acknowledging suspension.
    pub fn close(&self) -> usize {
        self.credits.lock(|state| {
            let mut state = state.borrow_mut();
            if state.open {
                state.open = false;
                state.close_generation = state
                    .close_generation
                    .checked_add(1)
                    .expect("open observation ingress generation exhausted");
            }
            let mut drained = 0;
            while self.channel.try_receive().is_ok() {
                drained += 1;
            }
            state.outstanding = state
                .outstanding
                .checked_sub(drained)
                .expect("observation request credit underflow on close");
            drained
        })
    }

    /// Resume admission after the consumer has completed suspension/resumption.
    /// Existing outstanding credits are preserved, never reset. Generation
    /// exhaustion permanently closes admission rather than aliasing old entries.
    pub fn open(&self) -> bool {
        self.credits.lock(|state| {
            let mut state = state.borrow_mut();
            if state.close_generation == u32::MAX {
                return false;
            }
            state.open = true;
            true
        })
    }

    /// An admitted entry with an older generation was cancelled by suspension.
    pub fn close_generation(&self) -> u32 {
        self.credits.lock(|state| state.borrow().close_generation)
    }

    pub fn outstanding(&self) -> usize {
        self.credits.lock(|state| state.borrow().outstanding)
    }
}

impl<M: RawMutex, F: FieldMask, const N: usize> Default for RequestIngress<M, F, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
