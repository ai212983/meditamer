//! One provider peek per display iteration. Accepted observations update the
//! backend cache; normal input/timer rendering owns physical refresh. Sampling
//! and cache delivery remain independent of upload/panel repaint policy.

use observation::delivery::{decide_delivery, DeliveryAction, DeliveryInput};
use observation::demand::{aggregate_demand, Demand};
use observation::ids::{
    OwnerGeneration, OwnerGenerationExhausted, OwnerId, ProviderGeneration, Revision,
};
use observation::observe_now::ObserveNowRequest;
use observation::policy::Health;
use observation::schedule::FieldTimestamps;
use observation::subscription::{DeliveryTracking, SubscriptionBook, SubscriptionKey};
use observation::time::Instant as ObsInstant;
use shell::types::SurfaceInstanceToken;

use crate::firmware::environment::{
    self, EnvironmentFields, EnvironmentStateSnapshot, RequestAdmissionError, FIELDS,
    SUBSCRIPTION_CAPACITY,
};

/// The UI owns subscription identity and delivery. The second book slot is
/// reserved for a fixture consumer; provider transport owns admitted requests.
#[derive(Default)]
pub(super) struct EnvironmentDeliveryState {
    owner: Option<SurfaceInstanceToken>,
    generation: OwnerGeneration,
    generation_exhausted: bool,
    key: Option<SubscriptionKey>,
    book: SubscriptionBook<EnvironmentFields, SUBSCRIPTION_CAPACITY>,
    entry: Option<EntryRefresh>,
    last_health: Option<(ProviderGeneration, Health)>,
    fixture: super::observation_fixture::FixtureState<EnvironmentStateSnapshot>,
}

struct EntryRefresh {
    expires_at: ObsInstant,
    admission: Option<EntryAdmission>,
    last_rejection: Option<RequestAdmissionError>,
}

struct EntryAdmission {
    admitted_at: ObsInstant,
    baseline: Option<(ProviderGeneration, Revision)>,
    close_generation: u32,
}

impl EntryRefresh {
    fn satisfied_by(&self, state: EnvironmentStateSnapshot) -> bool {
        let Some(admission) = &self.admission else {
            return false;
        };
        state.last_sample_at.is_some_and(|at| {
            state
                .sample_identity()
                .is_some_and(|identity| Some(identity) != admission.baseline)
                && at >= admission.admitted_at
                && at < self.expires_at
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryAdmissionEvent {
    Admitted,
    Rejected(RequestAdmissionError),
    Expired,
}

pub(super) struct EnvironmentDelivery {
    pub state: EnvironmentStateSnapshot,
    pub deliver_values: bool,
    pub health_changed: bool,
}

impl EnvironmentDeliveryState {
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

    pub(super) fn fixture_pending(&self) -> bool {
        self.fixture.is_pending()
    }

    pub(super) fn begin_fixture(
        &mut self,
        request: crate::firmware::observation_fixture::FixtureRequest,
        now: ObsInstant,
        state: Option<EnvironmentStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<EnvironmentFields>,
            ObsInstant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<crate::firmware::observation_fixture::FixtureResult<EnvironmentStateSnapshot>> {
        self.fixture
            .begin(request, now, state, close_generation, admit)
    }

    pub(super) fn poll_fixture(
        &mut self,
        now: ObsInstant,
        state: Option<EnvironmentStateSnapshot>,
        close_generation: u32,
    ) -> Option<crate::firmware::observation_fixture::FixtureResult<EnvironmentStateSnapshot>> {
        self.fixture.poll(now, state, close_generation)
    }

    pub(super) fn reconcile(
        &mut self,
        owner: Option<SurfaceInstanceToken>,
        now: ObsInstant,
    ) -> Result<bool, OwnerGenerationExhausted> {
        if self.owner == owner {
            return Ok(false);
        }
        if let Some(key) = self.key.take() {
            self.book.remove(key);
        }
        self.owner = None;
        self.entry = None;
        self.last_health = None;
        let Some(owner) = owner else {
            return Ok(true);
        };
        // Shell generations can coincide across provider replacement. This
        // authority renews its own identity for every exact-token change.
        self.generation = self.generation.checked_advance().inspect_err(|_| {
            self.generation_exhausted = true;
        })?;
        let mut subscription = environment::ambient_home_subscription();
        subscription.key.owner = OwnerId(owner.surface.id.0);
        subscription.key.owner_generation = self.generation;
        // A live owner replaces its predecessor; one of the two slots is
        // always available. Fixture admission must respect its reserved slot.
        self.book
            .upsert(subscription)
            .expect("reserved Home subscription slot");
        self.key = Some(subscription.key);
        self.owner = Some(owner);
        self.entry = Some(EntryRefresh {
            // Product validity horizon, not an acquisition latency guarantee.
            expires_at: now.saturating_add(subscription.max_age),
            admission: None,
            last_rejection: None,
        });
        Ok(true)
    }

    pub(super) fn environment_demand(&self, now: ObsInstant) -> Demand<EnvironmentFields, FIELDS> {
        aggregate_demand(&self.book, environment::ENVIRONMENT_PROVIDER_ID, now)
    }

    /// Admission retries retain the original entry expiry. Closure returns only
    /// after transport has accepted or explicitly rejected the request.
    pub(super) fn admit_entry(
        &mut self,
        now: ObsInstant,
        provider_state: Option<EnvironmentStateSnapshot>,
        close_generation: u32,
        admit: impl FnOnce(
            ObserveNowRequest<EnvironmentFields>,
            ObsInstant,
        ) -> Result<(), RequestAdmissionError>,
    ) -> Option<EntryAdmissionEvent> {
        let provider_state = provider_state.filter(EnvironmentStateSnapshot::is_environment);
        let entry = self.entry.as_mut()?;
        if entry
            .admission
            .as_ref()
            .is_some_and(|admission| admission.close_generation != close_generation)
        {
            // Suspension discarded this request. Retry under the same entry
            // lifetime after admission reopens, taking a new cache baseline.
            entry.admission = None;
            entry.last_rejection = None;
        }
        // A success completed within the request lifetime remains valid even
        // if the UI observes it after expiry. Failure revisions retain the
        // sample timestamp and cannot satisfy the request.
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
                    baseline: provider_state.and_then(|state| state.sample_identity()),
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

    pub(super) fn take_delivery(
        &mut self,
        owner: SurfaceInstanceToken,
        provider_state: Option<EnvironmentStateSnapshot>,
        now: ObsInstant,
    ) -> Option<EnvironmentDelivery> {
        if self.owner != Some(owner) {
            return None;
        }
        let state = provider_state
            .filter(|state| state.provider == environment::ENVIRONMENT_PROVIDER_ID)?;
        let subscription = *self.book.get(self.key?)?;
        let tracking = self.book.tracking_mut(subscription.key)?;
        if let Some((generation, _)) = self.last_health {
            if state.generation < generation {
                return None;
            }
            if state.generation != generation {
                *tracking = DeliveryTracking::default();
            }
        }
        let health_changed = self.last_health != Some((state.generation, state.health));
        let entry_satisfied = self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.satisfied_by(state));
        // Core freshness alone is insufficient: a fresh pre-entry cache or a
        // failed acquisition revision must not spend the admitted-result bypass.
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
            // Validity is checked at completion, not presentation. An explicit
            // result gets its bypass even if the UI observes it after max_age.
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
        Some(EnvironmentDelivery {
            state,
            deliver_values,
            health_changed,
        })
    }
}

#[cfg(target_os = "none")]
pub(super) fn reconcile_environment_owner(state: &mut super::state::DisplayLoopState) {
    let owner = state
        .presentation
        .backend
        .as_ref()
        .and_then(|backend| backend.committed_environment_owner());
    let now = ObsInstant(embassy_time::Instant::now().as_millis());
    let was_exhausted = state.environment_delivery.generation_exhausted;
    match state.environment_delivery.reconcile(owner, now) {
        Ok(false) => return,
        Ok(true) => {}
        Err(_) if !was_exhausted => {
            console::println!("BME688_OWNER rejected=generation_exhausted");
        }
        Err(_) => return,
    }
    environment::ENVIRONMENT_DEMAND.publish(state.environment_delivery.environment_demand(now));
}

#[cfg(target_os = "none")]
pub(super) fn poll_environment(state: &mut super::state::DisplayLoopState) {
    reconcile_environment_owner(state);
    let now = ObsInstant(embassy_time::Instant::now().as_millis());
    let provider_state = environment::ENVIRONMENT_STATE.try_get();
    let close_generation = environment::ENVIRONMENT_REQUESTS.close_generation();
    // The fixture is serviced even without an Ambient Home owner.
    service_fixture(
        &mut state.environment_delivery,
        now,
        provider_state,
        close_generation,
    );
    let Some(owner) = state.environment_delivery.owner else {
        return;
    };
    // Keep the observation's health and sample timestamp available to the
    // PMV check even when no new footer value qualifies for delivery. This
    // only updates backend state; it never composes or refreshes the panel.
    if let Some(snapshot) = provider_state.filter(EnvironmentStateSnapshot::is_environment) {
        super::presentation::update_environment_state(&mut state.presentation, snapshot);
    }
    if let Some(event) = state.environment_delivery.admit_entry(
        now,
        provider_state,
        environment::ENVIRONMENT_REQUESTS.close_generation(),
        |request, at| environment::ENVIRONMENT_REQUESTS.try_admit(request, at),
    ) {
        console::println!(
            "BME688_ENTRY event={:?} owner_generation={} at_ms={}",
            event,
            state.environment_delivery.generation.0,
            now.0,
        );
    }
    let Some(delivery) = state
        .environment_delivery
        .take_delivery(owner, provider_state, now)
    else {
        return;
    };
    if delivery.health_changed {
        console::println!(
            "BME688_HEALTH health={:?} generation={} revision={} has_sample={} owner_generation={}",
            delivery.state.health,
            delivery.state.generation.0,
            delivery.state.revision.0,
            delivery.state.last_sample_at.is_some(),
            owner.generation.0,
        );
    }
    if !delivery.deliver_values {
        return;
    }
    let delivery = delivery.state;
    if cfg!(feature = "firmware-trace") {
        console::println!(
        "BME688_DELIVER temperature_centidegrees={} humidity_millipercent={} sht45_temperature_centidegrees={:?} sht45_humidity_millipercent={:?} health={:?} generation={} revision={} last_sample_at_ms={:?} last_attempt_at_ms={:?} last_sample_revision={:?}",
        delivery.snapshot.onboard.temperature_centidegrees,
        delivery.snapshot.onboard.humidity_millipercent,
        delivery.snapshot.external.map(|reading| reading.temperature_centidegrees),
        delivery.snapshot.external.map(|reading| reading.humidity_millipercent),
        delivery.health,
        delivery.generation.0,
        delivery.revision.0,
        delivery.last_sample_at.map(|at| at.0),
        delivery.last_attempt_at.map(|at| at.0),
        delivery.last_sample_revision.map(|revision| revision.0),
    );
    }
    super::presentation::update_environment(&mut state.presentation, delivery.snapshot);
}

#[cfg(target_os = "none")]
fn service_fixture(
    authority: &mut EnvironmentDeliveryState,
    now: ObsInstant,
    state: Option<EnvironmentStateSnapshot>,
    close_generation: u32,
) {
    use crate::firmware::observation_fixture::{self as fixture, FixtureProvider};
    if let Some(result) = authority.poll_fixture(now, state, close_generation) {
        fixture::log_result(result);
    }
    if let Some(command) = fixture::try_receive(FixtureProvider::Bme688) {
        let Some(request) = fixture::route_command::<
            EnvironmentStateSnapshot,
            { crate::firmware::environment::FIELDS },
        >(
            command,
            &crate::firmware::environment::ENVIRONMENT_DEMAND,
            now,
            close_generation,
            authority.fixture_pending(),
        ) else {
            return;
        };
        if let Some(result) =
            authority.begin_fixture(request, now, state, close_generation, |observe, at| {
                environment::ENVIRONMENT_REQUESTS.try_admit(observe, at)
            })
        {
            fixture::log_result(result);
        }
    }
}
