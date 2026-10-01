//! Phase 2: the generic Embassy provider loop.
//!
//! This module is the reusable, host-testable *reactive step* a target's
//! provider task drives -- not the task itself. Per the plan's Ownership
//! section, targets keep the actual `#[embassy_executor::task]`, the static
//! `embassy_sync::watch::Watch`/`Channel` instances, and the concrete
//! driver; a target task loop looks roughly like:
//!
//! ```text
//! loop {
//!     match select(demand_watch.receiver().changed(), requests.receive()).await {
//!         Either::First(demand) => current_demand = demand,
//!         Either::Second(request) => { let _ = provider.admit_observe_now(request); }
//!     }
//!     let outcome = provider.step(&current_demand, || Instant::now()).await;
//!     state_watch.sender().send(provider.state_snapshot());
//!     // schedule the next wake at outcome's next_wake/backoff_until, or wait idle.
//! }
//! ```
//!
//! `step` performs at most one acquisition per call and never blocks
//! waiting for new demand or requests itself -- the caller's `select`
//! around the task loop is what waits idle when demand and pending requests
//! are empty. A target can accept requests into its bounded transport while
//! `step` borrows this loop for an acquisition. Immediately after `step`,
//! synchronously drain that transport through
//! [`ProviderLoop::admit_observe_now_after_step`]: a compatible request admitted
//! before completion can use that conversion, while other requests remain
//! queued. Transport admission, control priority, and suspension belong to
//! the target's concrete `RawMutex`/executor pairing.

use crate::demand::Demand;
use crate::field::FieldMask;
use crate::ids::{ProviderGeneration, ProviderId, Revision, RevisionExhausted};
use crate::observe_now::{ObserveNowQueue, ObserveNowRequest, QueueFull};
use crate::schedule::plan_acquisition;
use crate::state::ObservationState;
use crate::time::{Duration, Instant};

/// A provider's hardware acquisition, owned by board/target code. Kept
/// generic and `async` so a target's real driver and a test's fake driver
/// implement the same interface.
///
/// `async fn` in a public trait normally warns because it cannot state an
/// auto-trait bound like `Send` -- irrelevant here, matching
/// `platform/time/wall-clock`'s `RtcBackend`: every executor in this workspace is
/// single-threaded, so no implementation ever needs `Send`.
#[allow(async_fn_in_trait)]
pub trait AcquisitionDriver<F: FieldMask, S> {
    type Error;

    /// Acquires at least `requested`'s fields; a combined operation may
    /// measure more (e.g. SHTC3 always returns both temperature and
    /// humidity). Returns the fields actually measured and the resulting
    /// full snapshot.
    async fn acquire(&mut self, requested: F) -> Result<(F, S), Self::Error>;

    /// Releases any peripheral state left mid-acquisition. Called on
    /// suspension; a driver with nothing to release returns `Ok` at once.
    async fn cancel(&mut self) -> Result<(), Self::Error>;

    /// The minimum spacing this driver/bus/target allows between the start
    /// of one acquisition and the next, regardless of demand.
    fn min_acquisition_interval(&self) -> Duration;
}

/// Doubles the previous backoff (starting from one minimum interval),
/// capped so it cannot overflow or grow unbounded across many consecutive
/// failures.
fn next_backoff(min_interval: Duration, previous: Duration) -> Duration {
    const CAP: u32 = 10 * 60 * 1000; // 10 minutes, in caller ticks (target: ms)
    let base = if previous.0 == 0 {
        min_interval.0.max(1)
    } else {
        previous.0
    };
    Duration(base.saturating_mul(2).min(CAP))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome<F: FieldMask> {
    /// Nothing was due and no request was pending; the caller should wait
    /// at least until `next_wake` (or indefinitely if `None`) before the
    /// next `step`.
    Idle { next_wake: Option<Instant> },
    /// Suspended; `step` performs no acquisition until [`ProviderLoop::resume`].
    Suspended,
    /// An acquisition succeeded.
    Acquired { fields: F, revision: Revision },
    /// An acquisition failed; the caller should wait until `retry_at`.
    AcquisitionFailed {
        revision: Revision,
        retry_at: Instant,
    },
    /// The revision counter is exhausted (target policy decides what this
    /// means -- typically a provider restart).
    RevisionExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendOutcome {
    /// Admission closed, pending one-shots discarded, driver cleanup
    /// succeeded (or there was nothing to clean up).
    Quiesced,
    /// Admission closed and one-shots discarded, but the driver's cleanup
    /// failed -- the caller must not treat the peripheral as safely
    /// released.
    CleanupFailed,
}

/// Whether an accepted request still needs an acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestAdmission {
    Queued,
    Satisfied,
}

/// Explicit rejection for a target's bounded request transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestRejection {
    Suspended,
    Expired,
    /// Empty fields or an admission stamp later than the supplied clock.
    Invalid,
    Full,
}

/// Drives one provider's acquisition loop against a caller-supplied
/// [`AcquisitionDriver`], `N` bounding how many observe-now requests may be
/// admitted at once and `FIELDS` matching the [`Demand`] this loop is
/// stepped against (see [`Demand`]'s doc comment).
pub struct ProviderLoop<
    F: FieldMask,
    S,
    D: AcquisitionDriver<F, S>,
    const N: usize,
    const FIELDS: usize,
> {
    driver: D,
    state: ObservationState<S, FIELDS>,
    observe_now: ObserveNowQueue<F, N>,
    last_acquisition_started_at: Option<Instant>,
    retry_backoff: Duration,
    suspended: bool,
}

impl<F, S, D, const N: usize, const FIELDS: usize> ProviderLoop<F, S, D, N, FIELDS>
where
    F: FieldMask,
    D: AcquisitionDriver<F, S>,
{
    pub fn new(
        driver: D,
        provider: ProviderId,
        generation: ProviderGeneration,
        initial_snapshot: S,
    ) -> Self {
        Self {
            driver,
            state: ObservationState::new(provider, generation, initial_snapshot),
            observe_now: ObserveNowQueue::new(),
            last_acquisition_started_at: None,
            retry_backoff: Duration::ZERO,
            suspended: false,
        }
    }

    /// A read-only view of the latest state, for publishing to the state
    /// `Watch`.
    pub fn state(&self) -> &ObservationState<S, FIELDS> {
        &self.state
    }

    /// Admits an observe-now request. Rejected while suspended -- "close
    /// admission" -- as well as when the queue is already at capacity.
    pub fn admit_observe_now(&mut self, request: ObserveNowRequest<F>) -> Result<(), QueueFull> {
        if self.suspended {
            return Err(QueueFull);
        }
        self.observe_now.admit(request)
    }

    /// Admits a transport request after freeing expired slots. Callers retain
    /// the original transport admission stamp rather than retiming a request
    /// when the provider eventually drains it.
    pub fn admit_observe_now_at(
        &mut self,
        request: ObserveNowRequest<F>,
        now: Instant,
    ) -> Result<RequestAdmission, RequestRejection> {
        self.validate_request(&request, now)?;
        self.queue_request(request, now)
    }

    /// Drains a request accepted by target transport during the just-finished
    /// `step`. Call synchronously after that step, before awaiting another
    /// event or admitting newly arriving requests: millisecond stamps alone
    /// cannot order an arrival after completion within the same tick.
    ///
    /// Only that successful step's measured fields may satisfy the request;
    /// cached success, failed attempts, and stale outcomes cannot do so.
    pub fn admit_observe_now_after_step(
        &mut self,
        request: ObserveNowRequest<F>,
        outcome: StepOutcome<F>,
        now: Instant,
    ) -> Result<RequestAdmission, RequestRejection> {
        self.validate_request(&request, now)?;
        if let StepOutcome::Acquired { fields, revision } = outcome {
            if revision == self.state.revision
                && fields.contains(request.fields)
                && self.state.last_attempt_at.is_some_and(|completed_at| {
                    request.admitted_at <= completed_at && completed_at < request.expires_at
                })
            {
                return Ok(RequestAdmission::Satisfied);
            }
        }
        self.queue_request(request, now)
    }

    fn queue_request(
        &mut self,
        request: ObserveNowRequest<F>,
        now: Instant,
    ) -> Result<RequestAdmission, RequestRejection> {
        if request.expires_at <= now {
            return Err(RequestRejection::Expired);
        }
        self.observe_now
            .admit(request)
            .map(|()| RequestAdmission::Queued)
            .map_err(|_| RequestRejection::Full)
    }

    fn validate_request(
        &mut self,
        request: &ObserveNowRequest<F>,
        now: Instant,
    ) -> Result<(), RequestRejection> {
        self.observe_now.purge_expired(now);
        if self.suspended {
            Err(RequestRejection::Suspended)
        } else if request.fields.is_empty() || request.admitted_at > now {
            Err(RequestRejection::Invalid)
        } else {
            Ok(())
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    pub fn pending_observe_now(&self) -> usize {
        self.observe_now.len()
    }

    fn idle_until(&self, next_wake: Option<Instant>) -> StepOutcome<F> {
        StepOutcome::Idle {
            next_wake: next_wake
                .into_iter()
                .chain(self.observe_now.next_expiry())
                .min(),
        }
    }

    /// One reactive iteration: purges expired requests, then performs at
    /// most one acquisition covering whatever due periodic demand and
    /// admitted observe-now requests are outstanding, coalesced into a
    /// single driver call.
    ///
    /// `now` is a clock closure, not a single `Instant`, because acquisition
    /// itself awaits real conversion/bus latency: this crate's rule is never
    /// to read the clock inside pure logic, but a single pre-await `Instant`
    /// passed in by the caller would silently backdate the field stamp and
    /// retry deadline this call produces to *before* the conversion that
    /// produced them started, understating both by however long the
    /// conversion actually took. Called once before the driver call (to
    /// decide what, if anything, is due) and once more immediately after it
    /// resolves (to stamp the result and compute retry timing against when
    /// the attempt actually completed).
    pub async fn step(
        &mut self,
        demand: &Demand<F, FIELDS>,
        now: impl Fn() -> Instant,
    ) -> StepOutcome<F> {
        if self.suspended {
            return StepOutcome::Suspended;
        }
        let started_at = now();
        self.observe_now.purge_expired(started_at);

        let plan = plan_acquisition(demand, &self.state.field_timestamps, started_at);
        let observe_now_fields = self.observe_now.pending_fields();
        let requested = plan.due_now.union(observe_now_fields);

        if requested.is_empty() {
            return self.idle_until(plan.next_deadline);
        }

        let spacing_deadline = self
            .last_acquisition_started_at
            .map(|started| started.saturating_add(self.driver.min_acquisition_interval()));
        let retry_deadline = self
            .state
            .last_attempt_at
            .filter(|_| self.retry_backoff != Duration::ZERO)
            .map(|completed| completed.saturating_add(self.retry_backoff));
        if let Some(earliest_next) = spacing_deadline.max(retry_deadline) {
            if started_at < earliest_next {
                return self.idle_until(Some(earliest_next));
            }
        }

        let conversion_started_at = started_at;
        self.last_acquisition_started_at = Some(conversion_started_at);

        match self.driver.acquire(requested).await {
            Ok((measured, snapshot)) => {
                let completed_at = now();
                self.retry_backoff = Duration::ZERO;
                self.observe_now.take_satisfied(measured, completed_at);
                match self
                    .state
                    .apply_success(measured, completed_at, |s| *s = snapshot)
                {
                    Ok(()) => StepOutcome::Acquired {
                        fields: measured,
                        revision: self.state.revision,
                    },
                    Err(RevisionExhausted) => StepOutcome::RevisionExhausted,
                }
            }
            Err(_driver_error) => {
                let completed_at = now();
                self.observe_now.purge_expired(completed_at);
                self.retry_backoff =
                    next_backoff(self.driver.min_acquisition_interval(), self.retry_backoff);
                match self.state.apply_failure(completed_at) {
                    Ok(()) => StepOutcome::AcquisitionFailed {
                        revision: self.state.revision,
                        retry_at: completed_at.saturating_add(self.retry_backoff),
                    },
                    Err(RevisionExhausted) => StepOutcome::RevisionExhausted,
                }
            }
        }
    }

    /// Closes admission, discards pending one-shots, and asks the driver to
    /// release any peripheral state left mid-acquisition. Periodic demand
    /// itself is untouched -- it lives in the caller's demand book, not
    /// here -- so [`ProviderLoop::resume`] alone is enough to pick it back
    /// up. Available even when the observe-now queue is already full.
    pub async fn suspend(&mut self, now: Instant) -> SuspendOutcome {
        self.suspended = true;
        self.observe_now = ObserveNowQueue::new();
        self.last_acquisition_started_at = None;
        let _ = now;
        match self.driver.cancel().await {
            Ok(()) => SuspendOutcome::Quiesced,
            Err(_) => SuspendOutcome::CleanupFailed,
        }
    }

    /// Reopens admission and acquisition. The next `step` re-evaluates
    /// freshness against `demand` as normal -- there is no separate
    /// "forced refresh on resume" here; a caller that wants one issues an
    /// observe-now request before calling `step`.
    pub fn resume(&mut self) {
        self.suspended = false;
    }
}

#[cfg(test)]
mod tests;
