//! Field/snapshot types for the BQ27441 provider (ADR-0018, typed
//! observation subscriptions plan, Phase 5). Split out of the former
//! `battery.rs` the same way `environment::types` is, so `driver.rs` (host-
//! testable, no `console` dependency) can depend on just these plain types
//! rather than the whole module.

use observation::field::FieldMask;
use observation::ids::{ProviderGeneration, ProviderId, Revision};
use observation::policy::Health;
use observation::time::Instant;

/// Scoped to this crate's own subscription authority -- `1` is
/// `firmware::environment`'s `ENVIRONMENT_PROVIDER_ID`, so this is `2`.
pub const BATTERY_PROVIDER_ID: ProviderId = ProviderId(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryFields(u32);

impl BatteryFields {
    pub const LEVEL: BatteryFields = BatteryFields(1 << 0);
    const KNOWN: u32 = Self::LEVEL.0;
}

impl FieldMask for BatteryFields {
    const EMPTY: Self = BatteryFields(0);

    fn bits(self) -> u32 {
        self.0
    }

    fn from_bits_truncate(bits: u32) -> Self {
        BatteryFields(bits & Self::KNOWN)
    }
}

/// This provider's declared field-bit capacity: [`BatteryFields`] only ever
/// sets `LEVEL` (bit 0), so `Demand` and `FieldTimestamps`'s per-field
/// storage need exactly one slot, not
/// `observation::field::SUGGESTED_MAX_FIELDS`'s generic four.
pub(crate) const FIELDS: usize = 1;
/// Persistent trace owner plus one bounded fixture subscription.
pub(crate) const SUBSCRIPTION_CAPACITY: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatterySnapshot {
    pub percent: u8,
}

/// Queryable latest state; only successful acquisition advances sample identity.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BatteryStateSnapshot {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub health: Health,
    pub last_attempt_at: Option<Instant>,
    pub last_sample_at: Option<Instant>,
    pub last_sample_revision: Option<Revision>,
    pub snapshot: BatterySnapshot,
}

impl BatteryStateSnapshot {
    pub(crate) fn sample_identity(&self) -> Option<(ProviderGeneration, Revision)> {
        self.last_sample_revision
            .map(|revision| (self.generation, revision))
    }
}
