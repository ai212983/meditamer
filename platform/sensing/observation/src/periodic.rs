//! Bounded diagnostic demand overrides. Live authority is never overwritten.
//! Only the provider calls `resolve`, between acquisitions; its wake deadline
//! includes expiry, so restoration does not require another UI poll.

use crate::{
    demand::Demand,
    field::FieldMask,
    fixture::{FixtureStatus, Optional},
    ids::ProviderId,
    time::{Duration, Instant},
};
use core::{cell::RefCell, fmt};
use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};

/// Diagnostic limits follow the existing fastest/slowest production cadences.
/// They bound fixture load/lifetime, not acquisition latency or early sampling.
pub const MIN_INTERVAL_MS: u32 = 60_000;
pub const MAX_INTERVAL_MS: u32 = 300_000;
pub const MAX_VALIDITY_MS: u32 = 3 * MAX_INTERVAL_MS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodicRequest {
    pub id: u64,
    pub interval: Duration,
    pub expires_at: Instant,
}
impl PeriodicRequest {
    pub fn valid(self, now: Instant) -> bool {
        self.id != 0
            && (MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&self.interval.0)
            && self.expires_at > now
            && self.expires_at <= now.saturating_add(Duration(MAX_VALIDITY_MS))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodicStatus {
    Applied,
    Restored,
    Cancelled,
    Closed,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
pub struct PeriodicEvent {
    pub request: PeriodicRequest,
    pub applied_at: Option<Instant>,
    pub at: Instant,
    pub status: PeriodicStatus,
}
impl PeriodicEvent {
    pub fn report<F: FieldMask, const N: usize>(
        self,
        provider: ProviderId,
        live: Demand<F, N>,
    ) -> EventReport<F, N> {
        EventReport {
            event: self,
            provider,
            live,
        }
    }
}
pub struct EventReport<F: FieldMask, const N: usize> {
    event: PeriodicEvent,
    provider: ProviderId,
    live: Demand<F, N>,
}
impl<F: FieldMask, const N: usize> fmt::Display for EventReport<F, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let e = self.event;
        write!(f, "OBSPER {} id={} provider={} status={:?} applied_at_ms={} at_ms={} expires_at_ms={} interval_ms={} live_fields={} live_age0_ms={} live_age1_ms={}",
            if e.status == PeriodicStatus::Applied { "APPLIED" } else { "RESULT" }, e.request.id, self.provider.0, e.status,
            Optional(e.applied_at.map(|at| at.0)), e.at.0, e.request.expires_at.0, e.request.interval.0, self.live.fields.bits(),
            Optional(self.live.max_age_at(0).map(|age| age.0)), Optional(self.live.max_age_at(1).map(|age| age.0)))
    }
}

#[derive(Clone, Copy)]
struct Lease<F> {
    request: PeriodicRequest,
    fields: F,
    epoch: u32,
    applied_at: Option<Instant>,
    cancelled: bool,
}
struct State<F: FieldMask, const N: usize> {
    live: Demand<F, N>,
    lease: Option<Lease<F>>,
    stopped: bool,
    closed: bool,
    last_id: u64,
}

pub struct DemandControl<M: RawMutex, F: FieldMask, const N: usize> {
    state: Mutex<M, RefCell<State<F, N>>>,
    changed: Signal<M, ()>,
}

pub struct Selection<F: FieldMask, const N: usize> {
    pub demand: Demand<F, N>,
    pub live: Demand<F, N>,
    pub event: Option<PeriodicEvent>,
    pub window: Option<PeriodicWindow>,
}
#[derive(Clone, Copy, Debug)]
pub struct PeriodicWindow {
    pub id: u64,
    pub applied_at: Instant,
    pub expires_at: Instant,
}
impl<F: FieldMask, const N: usize> Selection<F, N> {
    /// Expiry wakes even when demand is empty, rate-limited, or in backoff.
    pub fn deadline(&self, next: Option<Instant>) -> Option<Instant> {
        next.into_iter()
            .chain(self.window.map(|w| w.expires_at))
            .min()
    }
}

impl<M: RawMutex, F: FieldMask, const N: usize> Default for DemandControl<M, F, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<M: RawMutex, F: FieldMask, const N: usize> DemandControl<M, F, N> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                live: Demand::empty(),
                lease: None,
                stopped: false,
                closed: false,
                last_id: 0,
            })),
            changed: Signal::new(),
        }
    }
    /// Product authority continues publishing live changes throughout a fixture.
    pub fn publish(&self, live: Demand<F, N>) {
        self.state.lock(|cell| cell.borrow_mut().live = live);
        self.changed.signal(());
    }
    pub async fn changed(&self) {
        self.changed.wait().await;
    }
    pub fn live(&self) -> Demand<F, N> {
        self.state.lock(|cell| cell.borrow().live)
    }
    pub fn is_pending(&self) -> bool {
        self.state.lock(|cell| cell.borrow().lease.is_some())
    }
    /// Read-only application acknowledgement for an exact diagnostic request.
    /// Only `resolve` advances this state; observers cannot apply a lease.
    pub fn application(&self, id: u64) -> Option<bool> {
        self.state.lock(|cell| {
            cell.borrow()
                .lease
                .filter(|lease| lease.request.id == id)
                .map(|lease| lease.applied_at.is_some())
        })
    }
    /// Routes the shared fixture slot without modifying live subscriptions.
    pub fn command(
        &self,
        mode: crate::fixture::command::FixtureMode,
        request: crate::fixture::FixtureRequest,
        fields: F,
        now: Instant,
        epoch: u32,
        once_pending: bool,
    ) -> Result<Option<crate::fixture::FixtureRequest>, FixtureStatus> {
        use crate::fixture::command::FixtureMode;
        match mode {
            FixtureMode::Once => {
                if self.is_pending() {
                    Err(FixtureStatus::Busy)
                } else {
                    Ok(Some(request))
                }
            }
            FixtureMode::Periodic { interval_ms } => {
                if once_pending {
                    return Err(FixtureStatus::Busy);
                }
                self.begin(
                    PeriodicRequest {
                        id: request.id,
                        interval: Duration(interval_ms),
                        expires_at: request.expires_at,
                    },
                    fields,
                    now,
                    epoch,
                )?;
                Ok(None)
            }
            FixtureMode::Cancel => {
                self.cancel(request.id)?;
                Ok(None)
            }
        }
    }
    pub fn begin(
        &self,
        request: PeriodicRequest,
        fields: F,
        now: Instant,
        epoch: u32,
    ) -> Result<(), FixtureStatus> {
        let result = self.state.lock(|cell| {
            let mut s = cell.borrow_mut();
            if !request.valid(now)
                || fields.is_empty()
                || Demand::<F, N>::uniform(fields, request.interval)
                    .fields
                    .bits()
                    != fields.bits()
            {
                return Err(FixtureStatus::Invalid);
            }
            if request.id <= s.last_id {
                return Err(FixtureStatus::StaleId);
            }
            s.last_id = request.id;
            if s.stopped || s.closed {
                return Err(FixtureStatus::Closed);
            }
            if s.lease.is_some() {
                return Err(FixtureStatus::Busy);
            }
            s.lease = Some(Lease {
                request,
                fields,
                epoch,
                applied_at: None,
                cancelled: false,
            });
            Ok(())
        });
        if result.is_ok() {
            self.changed.signal(());
        }
        result
    }
    /// Cancellation addresses the original ID, never a newer session. Completion
    /// is emitted only after the provider settles its current acquisition.
    pub fn cancel(&self, id: u64) -> Result<(), FixtureStatus> {
        let result = self.state.lock(|cell| {
            let mut s = cell.borrow_mut();
            let lease = s
                .lease
                .as_mut()
                .filter(|l| l.request.id == id)
                .ok_or(FixtureStatus::StaleId)?;
            lease.cancelled = true;
            Ok(())
        });
        if result.is_ok() {
            self.changed.signal(());
        }
        result
    }
    pub fn resolve(&self, now: Instant, epoch: u32) -> Selection<F, N> {
        self.state.lock(|cell| {
            let mut s = cell.borrow_mut();
            // Only a running provider reopens periodic admission after control.
            s.closed = s.stopped;
            let mut selected = Selection {
                demand: s.live,
                live: s.live,
                event: None,
                window: None,
            };
            let Some(mut lease) = s.lease else {
                return selected;
            };
            let status = if lease.epoch != epoch {
                Some(PeriodicStatus::Closed)
            } else if lease.cancelled {
                Some(PeriodicStatus::Cancelled)
            } else if now >= lease.request.expires_at {
                Some(PeriodicStatus::Restored)
            } else {
                None
            };
            if let Some(status) = status {
                s.lease = None;
                selected.event = Some(lease.event(now, status));
                return selected;
            }
            if lease.applied_at.is_none() {
                lease.applied_at = Some(now);
                s.lease = Some(lease);
                selected.event = Some(lease.event(now, PeriodicStatus::Applied));
            }
            selected.demand = Demand::uniform(lease.fields, lease.request.interval);
            selected.window = Some(PeriodicWindow {
                id: lease.request.id,
                applied_at: lease.applied_at.unwrap(),
                expires_at: lease.request.expires_at,
            });
            selected
        })
    }
    /// Called at a settled control/stop boundary before an await that may park
    /// the provider. Live demand is retained even when cleanup later fails.
    pub fn end(&self, now: Instant, stopped: bool) -> Option<PeriodicEvent> {
        self.state.lock(|cell| {
            let mut s = cell.borrow_mut();
            s.stopped |= stopped;
            s.closed = true;
            s.lease.take().map(|lease| {
                lease.event(
                    now,
                    if stopped {
                        PeriodicStatus::Stopped
                    } else {
                        PeriodicStatus::Closed
                    },
                )
            })
        })
    }
}
impl<F> Lease<F> {
    fn event(self, at: Instant, status: PeriodicStatus) -> PeriodicEvent {
        PeriodicEvent {
            request: self.request,
            applied_at: self.applied_at,
            at,
            status,
        }
    }
}

/// Only successful in-window acquisitions are emitted as periodic evidence.
impl PeriodicWindow {
    pub fn deadline(window: Option<Self>, next: Option<Instant>) -> Option<Instant> {
        next.into_iter().chain(window.map(|w| w.expires_at)).min()
    }

    pub fn sample<S: crate::fixture::FixtureSample>(self, state: S) -> Option<PeriodicSample<S>> {
        let metadata = state.metadata();
        let at = metadata.last_sample_at?;
        (at >= self.applied_at && at < self.expires_at).then_some(PeriodicSample {
            window: self,
            state,
        })
    }
}
pub struct PeriodicSample<S> {
    window: PeriodicWindow,
    state: S,
}
impl<S: crate::fixture::FixtureSample> fmt::Display for PeriodicSample<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.state.metadata();
        write!(f, "OBSPER SAMPLE id={} provider={} fields={} generation={} revision={} sampled_at_ms={} health={:?}",
            self.window.id, s.provider.0, S::fields().bits(), s.generation.0, s.revision.0,
            Optional(s.last_sample_at.map(|at| at.0)), s.health)?;
        S::write_values(Some(self.state), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields as F;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    type Control = DemandControl<NoopRawMutex, F, 2>;
    fn request(id: u64) -> PeriodicRequest {
        PeriodicRequest {
            id,
            interval: Duration(60_000),
            expires_at: Instant(180_000),
        }
    }
    fn live() -> Demand<F, 2> {
        Demand::uniform(F::TEMPERATURE.union(F::HUMIDITY), Duration(300_000))
    }

    #[test]
    fn expiry_restores_latest_live_demand_without_ui_or_saved_snapshot() {
        let control = Control::new();
        control.publish(live());
        control
            .begin(request(1), F::TEMPERATURE, Instant(10), 0)
            .unwrap();
        let selected = control.resolve(Instant(20), 0);
        assert_eq!(selected.event.unwrap().status, PeriodicStatus::Applied);
        assert_eq!(selected.demand.max_age_at(0), Some(Duration(60_000)));
        assert_eq!(selected.demand.fields, F::TEMPERATURE);
        assert_eq!(
            selected.deadline(Some(Instant(600_000))),
            Some(Instant(180_000))
        );
        control.publish(Demand::uniform(F::HUMIDITY, Duration(120_000)));
        assert_eq!(
            control.resolve(Instant(179_999), 0).demand.fields,
            F::TEMPERATURE
        );
        // No more product/UI calls: the provider wakes at the deadline.
        let restored = control.resolve(Instant(180_000), 0);
        assert_eq!(restored.event.unwrap().status, PeriodicStatus::Restored);
        assert_eq!(restored.demand.fields, F::HUMIDITY);
        assert_eq!(restored.demand.max_age_at(1), Some(Duration(120_000)));
        assert!(restored.window.is_none());
        assert!(control.resolve(Instant(180_001), 0).event.is_none());
    }
    #[test]
    fn withdrawal_during_fixture_restores_idle_not_resurrected_home() {
        let control = Control::new();
        control.publish(live());
        control
            .begin(request(1), F::TEMPERATURE, Instant(0), 0)
            .unwrap();
        control.resolve(Instant(0), 0);
        control.publish(Demand::empty());
        assert!(!control.resolve(Instant(1), 0).demand.is_empty());
        let restored = control.resolve(Instant(180_000), 0);
        assert!(restored.demand.is_empty());
        assert_eq!(restored.deadline(None), None);
    }
    #[test]
    fn cancellation_and_old_close_epoch_never_apply_or_restore_over_a_successor() {
        let control = Control::new();
        control.publish(live());
        control
            .begin(request(1), F::TEMPERATURE, Instant(0), 0)
            .unwrap();
        control.cancel(1).unwrap();
        let result = control.resolve(Instant(10), 0).event.unwrap();
        assert_eq!(result.status, PeriodicStatus::Cancelled);
        assert!(result.applied_at.is_none());
        control
            .begin(request(2), F::TEMPERATURE, Instant(10), 0)
            .unwrap();
        assert_eq!(control.cancel(1), Err(FixtureStatus::StaleId));
        assert_eq!(
            control.resolve(Instant(20), 1).event.unwrap().status,
            PeriodicStatus::Closed
        );
        assert_eq!(control.live().fields, live().fields);
    }
    #[test]
    fn control_and_terminal_stop_restore_before_parking() {
        for stopped in [false, true] {
            let control = Control::new();
            control.publish(live());
            control
                .begin(request(1), F::TEMPERATURE, Instant(0), 0)
                .unwrap();
            control.resolve(Instant(5), 0);
            let event = control.end(Instant(10), stopped).unwrap();
            assert_eq!(
                event.status,
                if stopped {
                    PeriodicStatus::Stopped
                } else {
                    PeriodicStatus::Closed
                }
            );
            assert!(!control.is_pending());
            assert!(control.end(Instant(11), stopped).is_none());
            assert_eq!(
                control.resolve(Instant(12), 0).demand.max_age_at(0),
                Some(Duration(300_000))
            );
            assert_eq!(
                control.begin(request(2), F::TEMPERATURE, Instant(12), 0),
                if stopped {
                    Err(FixtureStatus::Closed)
                } else {
                    Ok(())
                }
            );
        }
    }
    #[test]
    fn bounded_admission_consumes_busy_ids_and_expired_pending_never_applies() {
        let control = Control::new();
        control
            .begin(request(1), F::TEMPERATURE, Instant(0), 0)
            .unwrap();
        assert_eq!(
            control.begin(request(2), F::TEMPERATURE, Instant(0), 0),
            Err(FixtureStatus::Busy)
        );
        let event = control.resolve(Instant(180_001), 0).event.unwrap();
        assert_eq!(event.status, PeriodicStatus::Restored);
        assert!(event.applied_at.is_none());
        assert_eq!(
            control.begin(request(2), F::TEMPERATURE, Instant(0), 0),
            Err(FixtureStatus::StaleId)
        );
        assert_eq!(
            control.begin(request(u64::MAX), F::TEMPERATURE, Instant(0), 0),
            Ok(())
        );
        control.cancel(u64::MAX).unwrap();
        control.resolve(Instant(1), 0);
        assert_eq!(
            control.begin(request(3), F::TEMPERATURE, Instant(2), 0),
            Err(FixtureStatus::StaleId)
        );
    }
    #[test]
    fn rejects_empty_unrepresentable_fields_and_unbounded_or_fast_requests() {
        for bad in [
            PeriodicRequest {
                id: 0,
                ..request(1)
            },
            PeriodicRequest {
                interval: Duration(59_999),
                ..request(1)
            },
            PeriodicRequest {
                interval: Duration(300_001),
                ..request(1)
            },
            PeriodicRequest {
                expires_at: Instant(900_001),
                ..request(1)
            },
            PeriodicRequest {
                expires_at: Instant(0),
                ..request(1)
            },
        ] {
            assert_eq!(
                Control::new().begin(bad, F::TEMPERATURE, Instant(0), 0),
                Err(FixtureStatus::Invalid)
            );
        }
        assert_eq!(
            Control::new().begin(request(1), F::EMPTY, Instant(0), 0),
            Err(FixtureStatus::Invalid)
        );
        let one = DemandControl::<NoopRawMutex, F, 1>::new();
        assert_eq!(
            one.begin(request(1), F::HUMIDITY, Instant(0), 0),
            Err(FixtureStatus::Invalid)
        );
    }
    #[test]
    fn fixture_modes_are_exclusive_and_control_parking_closes_admission() {
        use crate::fixture::{command::FixtureMode as Mode, FixtureRequest};
        let control = Control::new();
        let req = FixtureRequest {
            id: 1,
            expires_at: Instant(150_000),
        };
        let period = Mode::Periodic {
            interval_ms: 60_000,
        };
        assert_eq!(
            control.command(period, req, F::TEMPERATURE, Instant(0), 0, true),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(
            control.command(period, req, F::TEMPERATURE, Instant(0), 0, false),
            Ok(None)
        );
        assert_eq!(
            control.command(Mode::Once, req, F::TEMPERATURE, Instant(0), 0, false),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(
            control.command(Mode::Cancel, req, F::TEMPERATURE, Instant(1), 0, true),
            Ok(None)
        );
        assert_eq!(
            control.resolve(Instant(2), 0).event.unwrap().status,
            PeriodicStatus::Cancelled
        );
        assert_eq!(
            control.command(Mode::Once, req, F::TEMPERATURE, Instant(2), 0, false),
            Ok(Some(req))
        );
        control.end(Instant(3), false);
        assert_eq!(
            control.begin(request(2), F::TEMPERATURE, Instant(4), 0),
            Err(FixtureStatus::Closed)
        );
        control.resolve(Instant(5), 0);
        assert_eq!(
            control.begin(request(3), F::TEMPERATURE, Instant(6), 0),
            Ok(())
        );
    }
    #[test]
    fn updates_before_wait_and_dropped_waits_preserve_provider_wake() {
        use core::{
            future::Future,
            pin::pin,
            task::{Context, Poll, Waker},
        };
        let control = Control::new();
        let mut cx = Context::from_waker(Waker::noop());
        {
            let mut wait = pin!(control.changed());
            assert_eq!(wait.as_mut().poll(&mut cx), Poll::Pending);
        }
        control
            .begin(request(1), F::TEMPERATURE, Instant(0), 0)
            .unwrap();
        let mut wait = pin!(control.changed());
        assert_eq!(wait.as_mut().poll(&mut cx), Poll::Ready(()));
        control.publish(Demand::uniform(F::HUMIDITY, Duration(300_000)));
        control.cancel(1).unwrap();
        let mut wait = pin!(control.changed());
        assert_eq!(wait.as_mut().poll(&mut cx), Poll::Ready(()));
        assert_eq!(control.resolve(Instant(1), 0).demand.fields, F::HUMIDITY);
    }
}
