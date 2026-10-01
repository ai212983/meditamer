//! Home observation ownership and delivery, independent of target I/O and LVGL.
//!
//! Only committed surfaces enter this adapter. Preparing a candidate does not
//! change ownership. Rebuilding a retained Home after wake or rejected sleep
//! explicitly renews its observation epoch even when its shell token is unchanged.
//! Each Home entry has bounded fresh-result admission alongside periodic demand.
//! Targets provide request transport; successful acquisition identities fence delivery.

pub mod fixture;
pub mod sleep_fixture;
mod types;
pub use types::*;

use observation::delivery::{decide_delivery, DeliveryAction, DeliveryInput};
use observation::demand::{aggregate_demand, Demand};
use observation::field::FieldMask;
use observation::fixture::FixtureState;
use observation::ids::{
    OwnerGeneration, OwnerGenerationExhausted, OwnerId, ProviderGeneration, ProviderId, Revision,
    SubscriptionSlot,
};
use observation::ingress::RequestAdmissionError;
use observation::observe_now::ObserveNowRequest;
use observation::policy::{Health, InitialPolicy};
use observation::schedule::FieldTimestamps;
use observation::subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey};
use observation::time::{Duration, Instant};
use shell::types::SurfaceInstanceToken;

use crate::{catalogue, config::SAMPLE_INTERVAL_S};

/// A health-only update never fabricates a reading from a default snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery<S> {
    pub sample: Option<S>,
    pub health: Option<Health>,
}

struct Consumer<F: FieldMask> {
    subscriptions: SubscriptionBook<F, SUBSCRIPTION_CAPACITY>,
    key: Option<SubscriptionKey>,
    entry: Option<EntryRefresh>,
    presented: Option<(ProviderGeneration, Health)>,
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
    fn satisfied_by<S>(&self, state: &StateSnapshot<S>) -> bool {
        let Some(admission) = &self.admission else {
            return false;
        };
        let (Some(at), Some(identity)) = (state.last_sample_at, state.sample_identity()) else {
            return false;
        };
        Some(identity) != admission.baseline && at >= admission.admitted_at && at < self.expires_at
    }
}

/// One event per admission, rejection transition or expiry, suitable for bounded
/// target diagnostics. Repeated rejection for the same reason stays silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryAdmissionEvent {
    Admitted,
    Rejected(RequestAdmissionError),
    Expired,
}

impl<F: FieldMask> Consumer<F> {
    fn new() -> Self {
        Self {
            subscriptions: SubscriptionBook::new(),
            key: None,
            entry: None,
            presented: None,
        }
    }

    fn clear(&mut self) {
        if let Some(key) = self.key.take() {
            self.subscriptions.remove(key);
        }
        self.entry = None;
        self.presented = None;
    }

    fn install(
        &mut self,
        provider: ProviderId,
        generation: OwnerGeneration,
        fields: F,
        now: Instant,
    ) {
        self.clear();
        let key = SubscriptionKey {
            provider,
            owner: OwnerId(catalogue::HOME_SURFACE_ID.0),
            owner_generation: generation,
            slot: SubscriptionSlot(0),
        };
        self.subscriptions
            .upsert(ObservationSubscription {
                key,
                fields,
                max_age: Duration::from_secs(SAMPLE_INTERVAL_S as u32),
                initial_policy: InitialPolicy::CacheThenRefresh,
                min_delivery_interval: Duration::from_secs(SAMPLE_INTERVAL_S as u32),
                expires_at: None,
            })
            .expect("reserved Home subscription slot");
        self.key = Some(key);
        self.entry = Some(EntryRefresh {
            // Product validity horizon, not an acquisition latency promise.
            expires_at: now.saturating_add(Duration::from_secs(SAMPLE_INTERVAL_S as u32)),
            admission: None,
            last_rejection: None,
        });
    }

    fn key(&self) -> Option<SubscriptionKey> {
        self.key
    }

    fn admit<S>(
        &mut self,
        now: Instant,
        state: Option<StateSnapshot<S>>,
        close_generation: u32,
        admit: impl FnOnce(ObserveNowRequest<F>, Instant) -> Result<(), RequestAdmissionError>,
    ) -> Option<EntryAdmissionEvent> {
        let expected = self.key?.provider;
        let state = state.filter(|state| state.provider == expected);
        let entry = self.entry.as_mut()?;
        if entry
            .admission
            .as_ref()
            .is_some_and(|admission| admission.close_generation != close_generation)
        {
            // Suspension may occur entirely between UI polls. A cancelled
            // admission cannot consume a result or extend the original expiry.
            entry.admission = None;
            entry.last_rejection = None;
        }
        if state
            .as_ref()
            .is_some_and(|state| entry.satisfied_by(state))
        {
            return None;
        }
        if now >= entry.expires_at {
            self.entry = None;
            return Some(EntryAdmissionEvent::Expired);
        }
        if entry.admission.is_some() {
            return None;
        }
        let subscription = self.subscriptions.get(self.key?)?;
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
                    baseline: state.as_ref().and_then(StateSnapshot::sample_identity),
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

    fn poll<S: Copy>(
        &mut self,
        key: SubscriptionKey,
        state: StateSnapshot<S>,
        now: Instant,
    ) -> Option<Delivery<S>> {
        // Validate before changing any baseline: late delivery cannot affect
        // the replacement owner, including its first health projection.
        if self.key != Some(key) || state.provider != key.provider {
            return None;
        }
        let subscription = *self.subscriptions.get(key)?;
        if self
            .presented
            .is_some_and(|(generation, _)| state.generation < generation)
        {
            return None;
        }
        let generation_changed = self
            .presented
            .is_some_and(|(generation, _)| generation != state.generation);
        let health_changed = self.presented != Some((state.generation, state.health));
        if generation_changed {
            self.subscriptions
                .upsert(subscription)
                .expect("replacement uses its existing slot");
        }
        self.presented = Some((state.generation, state.health));
        let mut cache = FieldTimestamps::<FIELDS>::new();
        if let Some(at) = state.last_sample_at {
            cache.stamp(subscription.fields, at);
        }
        let entry_satisfied = self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.satisfied_by(&state));
        let tracking = self.subscriptions.tracking_mut(key)?;
        // Fresh cached entry and failure revisions cannot spend the explicit
        // result bypass. Only a new successful revision completed within its
        // lifetime can; multiple ADC successes may share one timestamp.
        tracking.refresh_pending = entry_satisfied;
        if entry_satisfied {
            tracking.bypass_used = false;
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
        let sample = matches!(
            action,
            DeliveryAction::DeliverCache(_) | DeliveryAction::DeliverCacheAndRequestRefresh { .. }
        )
        .then_some(state.snapshot);
        if sample.is_some() && entry_satisfied {
            self.entry = None;
        }
        let health = health_changed.then_some(state.health);
        (sample.is_some() || health.is_some()).then_some(Delivery { sample, health })
    }
}

/// Owns each provider's Home subscription plus a reserved fixture slot. Entry
/// intent belongs here; admitted requests remain with the target until completion
/// or expiry, even if Home leaves before they complete.
pub struct HomeObservations {
    active: Option<SurfaceInstanceToken>,
    generation: OwnerGeneration,
    environment: Consumer<EnvironmentFields>,
    battery: Consumer<BatteryFields>,
    environment_fixture: FixtureState<EnvironmentStateSnapshot>,
    battery_fixture: FixtureState<BatteryStateSnapshot>,
}

impl HomeObservations {
    pub fn new() -> Self {
        Self {
            active: None,
            generation: OwnerGeneration(0),
            environment: Consumer::new(),
            battery: Consumer::new(),
            environment_fixture: FixtureState::default(),
            battery_fixture: FixtureState::default(),
        }
    }

    /// Fixture sessions survive Home withdrawal/rebuild. Only provider closure,
    /// restart, completion or expiry terminates them; they never change demand.
    pub fn environment_fixture(&mut self) -> &mut FixtureState<EnvironmentStateSnapshot> {
        &mut self.environment_fixture
    }
    pub fn battery_fixture(&mut self) -> &mut FixtureState<BatteryStateSnapshot> {
        &mut self.battery_fixture
    }

    /// Call only after a successful shell commit. An unchanged commit is
    /// idempotent; a failed or rolled-back candidate must not be passed here.
    pub fn committed(
        &mut self,
        instance: SurfaceInstanceToken,
        now: Instant,
    ) -> Result<(), OwnerGenerationExhausted> {
        if !Self::is_home(instance) {
            self.deactivate();
            return Ok(());
        }
        if self.active == Some(instance) {
            return Ok(());
        }
        self.rebuilt(instance, now)
    }

    /// A rebuilt retained surface gets a new observation identity. Exhaustion
    /// leaves demand closed rather than reusing an old generation.
    pub fn rebuilt(
        &mut self,
        instance: SurfaceInstanceToken,
        now: Instant,
    ) -> Result<(), OwnerGenerationExhausted> {
        self.deactivate();
        if !Self::is_home(instance) {
            return Ok(());
        }
        self.generation = self.generation.checked_advance()?;
        self.environment.install(
            ENVIRONMENT_PROVIDER_ID,
            self.generation,
            EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
            now,
        );
        self.battery.install(
            BATTERY_PROVIDER_ID,
            self.generation,
            BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
            now,
        );
        self.active = Some(instance);
        Ok(())
    }

    pub fn deactivate(&mut self) {
        self.active = None;
        self.environment.clear();
        self.battery.clear();
    }

    pub fn environment_key(&self) -> Option<SubscriptionKey> {
        self.environment.key()
    }
    pub fn battery_key(&self) -> Option<SubscriptionKey> {
        self.battery.key()
    }

    pub fn environment_demand(&self, now: Instant) -> Demand<EnvironmentFields, FIELDS> {
        aggregate_demand(
            &self.environment.subscriptions,
            ENVIRONMENT_PROVIDER_ID,
            now,
        )
    }

    pub fn battery_demand(&self, now: Instant) -> Demand<BatteryFields, FIELDS> {
        aggregate_demand(&self.battery.subscriptions, BATTERY_PROVIDER_ID, now)
    }

    /// Attempt the current entry's fresh observation using a single state peek
    /// shared with delivery. Sample the transport closure generation before
    /// calling admission, so an intervening close is detected on the next poll.
    pub fn admit_environment(
        &mut self,
        now: Instant,
        state: Option<EnvironmentStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<EnvironmentFields>,
            Instant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<EntryAdmissionEvent> {
        self.environment.admit(now, state, close_generation, admit)
    }

    pub fn admit_battery(
        &mut self,
        now: Instant,
        state: Option<BatteryStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<BatteryFields>,
            Instant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<EntryAdmissionEvent> {
        self.battery.admit(now, state, close_generation, admit)
    }

    pub fn poll_environment(
        &mut self,
        key: SubscriptionKey,
        state: EnvironmentStateSnapshot,
        now: Instant,
    ) -> Option<Delivery<EnvironmentSnapshot>> {
        self.environment.poll(key, state, now)
    }

    pub fn poll_battery(
        &mut self,
        key: SubscriptionKey,
        state: BatteryStateSnapshot,
        now: Instant,
    ) -> Option<Delivery<BatterySnapshot>> {
        self.battery.poll(key, state, now)
    }

    fn is_home(instance: SurfaceInstanceToken) -> bool {
        instance.surface.id == catalogue::HOME_SURFACE_ID
            && instance.surface.owner.id == catalogue::BASE_PROVIDER_ID
    }
}

impl Default for HomeObservations {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
