//! Field/snapshot types for the BME688 provider.
//! Contract: [ADR-0018](../../../../../docs/architecture/0018-typed-observation-subscriptions.md).

use inkplate_tempera::environment::EnvironmentReading;
use observation::field::FieldMask;
use observation::ids::{
    OwnerGeneration, OwnerId, ProviderGeneration, ProviderId, Revision, SubscriptionSlot,
};
use observation::policy::{Health, InitialPolicy};
use observation::subscription::{ObservationSubscription, SubscriptionKey};
use observation::time::{Duration as ObsDuration, Instant as ObsInstant};

use super::config::ENVIRONMENT_SAMPLE_INTERVAL_S;

/// The first (and, today, only) `observation`-based provider this product
/// registers. Scoped to this crate's own subscription authority, not shared
/// with `targets/medinote-waveshare`'s numbering.
pub const ENVIRONMENT_PROVIDER_ID: ProviderId = ProviderId(1);

/// The BME688 always reports temperature and humidity from one forced-mode
/// conversion together, the same combined-conversion shape as Medinote's
/// SHTC3 fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironmentFields(u32);

impl EnvironmentFields {
    pub const TEMPERATURE: EnvironmentFields = EnvironmentFields(1 << 0);
    pub const HUMIDITY: EnvironmentFields = EnvironmentFields(1 << 1);
    const KNOWN: u32 = Self::TEMPERATURE.0 | Self::HUMIDITY.0;
}

impl FieldMask for EnvironmentFields {
    const EMPTY: Self = EnvironmentFields(0);

    fn bits(self) -> u32 {
        self.0
    }

    fn from_bits_truncate(bits: u32) -> Self {
        EnvironmentFields(bits & Self::KNOWN)
    }
}

/// This provider's declared field-bit capacity: [`EnvironmentFields`] only
/// ever sets `TEMPERATURE`/`HUMIDITY` (bits 0-1), so `Demand` and
/// `FieldTimestamps`'s per-field storage need exactly two slots, not
/// `observation::field::SUGGESTED_MAX_FIELDS`'s generic four.
pub(crate) const FIELDS: usize = 2;

/// One acquisition keeps the mandatory onboard BME688 and the optional
/// easyC-connected SHT45 together. They remain separate so calibration can
/// compare them without changing the established BME688 presentation policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvironmentSnapshot {
    pub onboard: EnvironmentReading,
    pub external: Option<EnvironmentReading>,
}

/// What `display`'s adapter needs to decide Ambient Home's delivery: a lean
/// projection of `observation::state::ObservationState`, matching
/// `targets/medinote-waveshare`'s `EnvironmentStateSnapshot` reasoning --
/// both fields are always sampled together, so one combined timestamp is
/// exact.
#[derive(Clone, Copy, Debug)]
pub struct EnvironmentStateSnapshot {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub health: Health,
    pub last_attempt_at: Option<ObsInstant>,
    pub last_sample_at: Option<ObsInstant>,
    pub last_sample_revision: Option<Revision>,
    pub snapshot: EnvironmentSnapshot,
}

impl EnvironmentStateSnapshot {
    pub(crate) fn is_environment(&self) -> bool {
        self.provider == ENVIRONMENT_PROVIDER_ID
    }

    pub(crate) fn sample_identity(&self) -> Option<(ProviderGeneration, Revision)> {
        self.last_sample_revision
            .map(|revision| (self.generation, revision))
    }

    pub(crate) fn from_observation(
        state: &observation::state::ObservationState<EnvironmentSnapshot, FIELDS>,
        last_sample_revision: Option<Revision>,
    ) -> Self {
        Self {
            provider: state.provider,
            generation: state.generation,
            revision: state.revision,
            health: state.health,
            last_attempt_at: state.last_attempt_at,
            last_sample_at: state.field_timestamps.get(0),
            last_sample_revision,
            snapshot: state.snapshot,
        }
    }
}

impl Default for EnvironmentStateSnapshot {
    fn default() -> Self {
        Self {
            provider: ENVIRONMENT_PROVIDER_ID,
            generation: ProviderGeneration::INITIAL,
            revision: Revision::INITIAL,
            health: Health::Unknown,
            last_attempt_at: None,
            last_sample_at: None,
            last_sample_revision: None,
            snapshot: EnvironmentSnapshot {
                onboard: EnvironmentReading {
                    temperature_centidegrees: 0,
                    humidity_millipercent: 0,
                },
                external: None,
            },
        }
    }
}

/// Shared Ambient Home policy. The product authority replaces the template key
/// with its committed surface identity and checked observation generation.
pub(crate) fn ambient_home_subscription() -> ObservationSubscription<EnvironmentFields> {
    ObservationSubscription {
        key: SubscriptionKey {
            provider: ENVIRONMENT_PROVIDER_ID,
            // Template only; the product authority supplies the actual owner.
            owner: OwnerId(1),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        max_age: ObsDuration::from_secs(ENVIRONMENT_SAMPLE_INTERVAL_S),
        initial_policy: InitialPolicy::CacheThenRefresh,
        min_delivery_interval: ObsDuration::from_secs(ENVIRONMENT_SAMPLE_INTERVAL_S),
        expires_at: None,
    }
}
