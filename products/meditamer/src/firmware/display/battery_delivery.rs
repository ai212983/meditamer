//! Persistent battery demand and IMU trace delivery owned by the UI task.
//! Navigation and panel repaint policy never alter this consumer's lifetime.

use observation::delivery::{decide_delivery, DeliveryAction, DeliveryInput};
use observation::demand::{aggregate_demand, Demand};
use observation::ids::{
    OwnerGeneration, OwnerGenerationExhausted, OwnerId, ProviderGeneration, Revision,
    SubscriptionSlot,
};
use observation::ingress::RequestAdmissionError;
use observation::observe_now::ObserveNowRequest;
use observation::policy::{Health, InitialPolicy};
use observation::schedule::FieldTimestamps;
use observation::subscription::{
    DeliveryTracking, ObservationSubscription, SubscriptionBook, SubscriptionKey,
};
use observation::time::{Duration, Instant};

use crate::firmware::battery::{
    BatteryFields, BatteryStateSnapshot, BATTERY_PROVIDER_ID, FIELDS, SUBSCRIPTION_CAPACITY,
};
use crate::firmware::config::BATTERY_INTERVAL_SECONDS;
use crate::firmware::observation_fixture::{FixtureRequest, FixtureResult};

/// Product-owned trace consumer, deliberately not a shell surface identity.
const IMU_TRACE_OWNER: OwnerId = OwnerId(0);

#[derive(Default)]
pub(super) struct BatteryDeliveryState {
    generation: OwnerGeneration,
    generation_exhausted: bool,
    key: Option<SubscriptionKey>,
    book: SubscriptionBook<BatteryFields, SUBSCRIPTION_CAPACITY>,
    entry: Option<EntryRefresh>,
    last_health: Option<(ProviderGeneration, Health)>,
    fixture: super::observation_fixture::FixtureState<BatteryStateSnapshot>,
}

struct EntryRefresh {
    expires_at: Instant,
    admission: Option<EntryAdmission>,
    last_rejection: Option<RequestAdmissionError>,
}

struct EntryAdmission {
    admitted_at: Instant,
    baseline: Option<(ProviderGeneration, Revision)>,
    close_generation: u32,
}

impl EntryRefresh {
    fn satisfied_by(&self, state: BatteryStateSnapshot) -> bool {
        let Some(admission) = &self.admission else {
            return false;
        };
        let (Some(at), Some(identity)) = (state.last_sample_at, state.sample_identity()) else {
            return false;
        };
        Some(identity) != admission.baseline && at >= admission.admitted_at && at < self.expires_at
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryAdmissionEvent {
    Admitted,
    Rejected(RequestAdmissionError),
    Expired,
}

pub(super) struct BatteryDelivery {
    pub state: BatteryStateSnapshot,
    pub deliver_values: bool,
    pub health_changed: bool,
}

impl BatteryDeliveryState {
    pub(super) fn needs_service_retry(&self) -> bool {
        self.entry.is_some() || self.fixture_pending()
    }

    #[cfg(test)]
    pub(super) fn with_generation_for_test(generation: OwnerGeneration) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }

    /// The target activates this once for the display task's lifetime, never
    /// on navigation. Explicit teardown withdraws only this persistent owner.
    pub(super) fn reconcile(
        &mut self,
        active: bool,
        now: Instant,
    ) -> Result<bool, OwnerGenerationExhausted> {
        if self.key.is_some() == active {
            return Ok(false);
        }
        if let Some(key) = self.key.take() {
            self.book.remove(key);
        }
        self.entry = None;
        self.last_health = None;
        if !active {
            return Ok(true);
        }
        self.generation = self.generation.checked_advance().inspect_err(|_| {
            self.generation_exhausted = true;
        })?;
        let key = SubscriptionKey {
            provider: BATTERY_PROVIDER_ID,
            owner: IMU_TRACE_OWNER,
            owner_generation: self.generation,
            slot: SubscriptionSlot(0),
        };
        let interval = Duration::from_secs(BATTERY_INTERVAL_SECONDS);
        self.book
            .upsert(ObservationSubscription {
                key,
                fields: BatteryFields::LEVEL,
                max_age: interval,
                initial_policy: InitialPolicy::CacheThenRefresh,
                min_delivery_interval: interval,
                expires_at: None,
            })
            .expect("reserved battery trace subscription slot");
        self.key = Some(key);
        self.entry = Some(EntryRefresh {
            // Startup validity horizon, not a conversion-latency promise.
            expires_at: now.saturating_add(interval),
            admission: None,
            last_rejection: None,
        });
        Ok(true)
    }

    pub(super) fn key(&self) -> Option<SubscriptionKey> {
        self.key
    }

    pub(super) fn battery_demand(&self, now: Instant) -> Demand<BatteryFields, FIELDS> {
        aggregate_demand(&self.book, BATTERY_PROVIDER_ID, now)
    }

    pub(super) fn admit_entry(
        &mut self,
        now: Instant,
        provider_state: Option<BatteryStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<BatteryFields>,
            Instant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<EntryAdmissionEvent> {
        if let Some(state) = &provider_state {
            if !self.accepts_state(state) {
                return None;
            }
        }
        let entry = self.entry.as_mut()?;
        if entry
            .admission
            .as_ref()
            .is_some_and(|admission| admission.close_generation != close_generation)
        {
            entry.admission = None;
            entry.last_rejection = None;
        }
        if provider_state.is_some_and(|state| entry.satisfied_by(state)) {
            return None;
        }
        if now >= entry.expires_at {
            self.entry = None;
            return Some(EntryAdmissionEvent::Expired);
        }
        if entry.admission.is_some() {
            return None;
        }
        let subscription = self.book.get(self.key?)?;
        let request = ObserveNowRequest {
            owner: subscription.key.owner,
            owner_generation: subscription.key.owner_generation,
            fields: subscription.fields,
            admitted_at: now,
            expires_at: entry.expires_at,
        };
        match admit(request, now) {
            Ok(()) => {
                entry.admission = Some(EntryAdmission {
                    admitted_at: now,
                    baseline: provider_state
                        .as_ref()
                        .and_then(BatteryStateSnapshot::sample_identity),
                    close_generation,
                });
                entry.last_rejection = None;
                Some(EntryAdmissionEvent::Admitted)
            }
            Err(error) if entry.last_rejection != Some(error) => {
                entry.last_rejection = Some(error);
                Some(EntryAdmissionEvent::Rejected(error))
            }
            Err(_) => None,
        }
    }

    pub(super) fn fixture_pending(&self) -> bool {
        self.fixture.is_pending()
    }

    pub(super) fn begin_fixture(
        &mut self,
        request: FixtureRequest,
        now: Instant,
        state: Option<BatteryStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<BatteryFields>,
            Instant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<FixtureResult<BatteryStateSnapshot>> {
        self.fixture
            .begin(request, now, state, close_generation, admit)
    }

    pub(super) fn poll_fixture(
        &mut self,
        now: Instant,
        state: Option<BatteryStateSnapshot>,
        close_generation: u32,
    ) -> Option<FixtureResult<BatteryStateSnapshot>> {
        self.fixture.poll(now, state, close_generation)
    }

    fn accepts_state(&self, state: &BatteryStateSnapshot) -> bool {
        state.provider == BATTERY_PROVIDER_ID
            && match self.last_health {
                Some((generation, _)) => state.generation >= generation,
                None => true,
            }
    }

    pub(super) fn take_delivery(
        &mut self,
        key: SubscriptionKey,
        provider_state: Option<BatteryStateSnapshot>,
        now: Instant,
    ) -> Option<BatteryDelivery> {
        if self.key != Some(key) {
            return None;
        }
        let state = provider_state?;
        if !self.accepts_state(&state) {
            return None;
        }
        let subscription = *self.book.get(key)?;
        let tracking = self.book.tracking_mut(key)?;
        if let Some((generation, _)) = self.last_health {
            if state.generation != generation {
                *tracking = DeliveryTracking::default();
            }
        }
        let health_changed = self.last_health != Some((state.generation, state.health));
        let entry_satisfied = self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.satisfied_by(state));
        // Repainting an unchanged revision must not spend the next delivery
        // interval just before a due conversion completes. Entry results and
        // health transitions still take their existing bypass paths.
        if !health_changed
            && !entry_satisfied
            && tracking.last_delivered_revision == Some(state.revision)
        {
            return None;
        }
        tracking.refresh_pending = entry_satisfied;
        if entry_satisfied {
            tracking.bypass_used = false;
        }
        let mut cache = FieldTimestamps::<FIELDS>::new();
        if let Some(at) = state.last_sample_at {
            cache.stamp(subscription.fields, at);
        }
        let action = decide_delivery(
            &subscription,
            tracking,
            &cache,
            DeliveryInput {
                now,
                health_changed,
                revision: state.revision,
            },
        );
        tracking.refresh_pending = false;
        let deliver_values = matches!(
            action,
            DeliveryAction::DeliverCache(_) | DeliveryAction::DeliverCacheAndRequestRefresh { .. }
        );
        if deliver_values && entry_satisfied {
            self.entry = None;
        }
        if !deliver_values && !health_changed {
            return None;
        }
        self.last_health = Some((state.generation, state.health));
        Some(BatteryDelivery {
            state,
            deliver_values,
            health_changed,
        })
    }
}

#[cfg(target_os = "none")]
pub(super) fn start_battery(state: &mut super::state::DisplayLoopState) {
    let now = Instant(embassy_time::Instant::now().as_millis());
    let was_exhausted = state.battery_delivery.generation_exhausted;
    match state.battery_delivery.reconcile(true, now) {
        Ok(false) => return,
        Ok(true) => {}
        Err(_) if !was_exhausted => {
            console::println!("BATTERY_OWNER rejected=generation_exhausted")
        }
        Err(_) => return,
    }
    crate::firmware::battery::BATTERY_DEMAND.publish(state.battery_delivery.battery_demand(now));
}

#[cfg(target_os = "none")]
pub(super) fn poll_battery(state: &mut super::state::DisplayLoopState) {
    use crate::firmware::{battery, imu};

    let now = Instant(embassy_time::Instant::now().as_millis());
    let provider_state = battery::BATTERY_STATE.try_get();
    let close_generation = battery::BATTERY_REQUESTS.close_generation();
    let key = state.battery_delivery.key();
    if let Some(key) = key {
        // Persistent startup admission gets the first available credit.
        if let Some(event) = state.battery_delivery.admit_entry(
            now,
            provider_state,
            close_generation,
            |request, at| battery::BATTERY_REQUESTS.try_admit(request, at),
        ) {
            console::println!(
                "BATTERY_ENTRY event={:?} owner_generation={} at_ms={}",
                event,
                key.owner_generation.0,
                now.0
            );
        }
    }
    service_fixture(
        &mut state.battery_delivery,
        now,
        provider_state,
        close_generation,
    );
    let Some(key) = key else {
        return;
    };
    let Some(delivery) = state
        .battery_delivery
        .take_delivery(key, provider_state, now)
    else {
        return;
    };
    if delivery.health_changed {
        console::println!("BATTERY_HEALTH health={:?} generation={} revision={} has_sample={} owner_generation={}",
            delivery.state.health, delivery.state.generation.0, delivery.state.revision.0,
            delivery.state.last_sample_at.is_some(), key.owner_generation.0);
    }
    if delivery.deliver_values {
        // A failure with valid cache republishes only the retained last good
        // level. Missing samples never replace the trace's unknown/default value.
        imu::publish_trace_context(imu::ImuTraceContext {
            battery_percent: i16::from(delivery.state.snapshot.percent),
        });
        console::println!(
            "BATTERY_DELIVER percent={} generation={} revision={} health={:?} last_sample_at_ms={:?} last_attempt_at_ms={:?} last_sample_revision={:?}",
            delivery.state.snapshot.percent,
            delivery.state.generation.0,
            delivery.state.revision.0,
            delivery.state.health,
            delivery.state.last_sample_at.map(|at| at.0),
            delivery.state.last_attempt_at.map(|at| at.0),
            delivery.state.last_sample_revision.map(|revision| revision.0),
        );
    }
}

#[cfg(target_os = "none")]
fn service_fixture(
    authority: &mut BatteryDeliveryState,
    now: Instant,
    state: Option<BatteryStateSnapshot>,
    close_generation: u32,
) {
    use crate::firmware::{
        battery::BATTERY_REQUESTS,
        observation_fixture::{self as fixture, FixtureProvider},
    };

    // Complete the previous session before handling another command. No live
    // subscription is required, and fixture work never changes trace tracking.
    if let Some(result) = authority.poll_fixture(now, state, close_generation) {
        fixture::log_result(result);
    }
    if let Some(command) = fixture::try_receive(FixtureProvider::Battery) {
        let Some(request) =
            fixture::route_command::<BatteryStateSnapshot, { crate::firmware::battery::FIELDS }>(
                command,
                &crate::firmware::battery::BATTERY_DEMAND,
                now,
                close_generation,
                authority.fixture_pending(),
            )
        else {
            return;
        };
        if let Some(result) =
            authority.begin_fixture(request, now, state, close_generation, |observe, at| {
                BATTERY_REQUESTS.try_admit(observe, at)
            })
        {
            fixture::log_result(result);
        }
    }
}
