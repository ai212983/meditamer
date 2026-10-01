//! Subscription identity, storage, and the bounded book of active demand.

use crate::field::FieldMask;
use crate::ids::{OwnerGeneration, OwnerId, ProviderId, Revision, SubscriptionSlot};
use crate::policy::InitialPolicy;
use crate::time::{Duration, Instant};

/// Identifies one subscription slot: which provider, which owner and
/// generation, and which logical slot for that owner (ADR-0018: "Key
/// subscriptions by provider, owner identity, owner generation, and logical
/// slot").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscriptionKey {
    pub provider: ProviderId,
    pub owner: OwnerId,
    pub owner_generation: OwnerGeneration,
    pub slot: SubscriptionSlot,
}

/// A live request for a provider's fields at some cadence and freshness.
#[derive(Clone, Copy, Debug)]
pub struct ObservationSubscription<F: FieldMask> {
    pub key: SubscriptionKey,
    pub fields: F,
    /// Freshness threshold for cached fields. Scheduling at this age does
    /// not guarantee that conversion or delivery completes by this time.
    pub max_age: Duration,
    pub initial_policy: InitialPolicy,
    /// Minimum spacing between deliveries to this subscription, after its
    /// one-time entry/observe-now bypass is consumed.
    pub min_delivery_interval: Duration,
    /// `None`: lives until explicitly removed (surface teardown, provider
    /// removal). `Some`: a background subscription with its own bounded
    /// lifetime, purged once `now` reaches it without needing an explicit
    /// remove call.
    pub expires_at: Option<Instant>,
}

/// Per-subscription delivery bookkeeping the book stores alongside the
/// subscription itself. See [`crate::delivery`].
#[derive(Clone, Copy, Debug, Default)]
pub struct DeliveryTracking {
    pub last_delivered_at: Option<Instant>,
    pub last_delivered_revision: Option<Revision>,
    /// Consumed by the first delivery (entry refresh or an observe-now
    /// result); every later delivery is rate-limited by
    /// `min_delivery_interval`.
    pub bypass_used: bool,
    /// A refresh this subscription is still waiting on: set by
    /// [`crate::delivery::decide_delivery`] whenever it asks for one
    /// (`RequestRefreshOnly`, or `DeliverCacheAndRequestRefresh`'s non-empty
    /// `refresh`), and by a caller that admits an explicit observe-now
    /// request on this subscription's behalf (`decide_delivery` has no
    /// visibility into the observe-now queue itself). The delivery that
    /// satisfies it bypasses `min_delivery_interval` once, then this clears
    /// -- "Entry refresh and explicit observe-now results bypass the
    /// interval once; later periodic values follow it."
    pub refresh_pending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookFull;

/// A fixed-capacity table of active subscriptions for one subscription
/// authority (one product UI owner, per the plan's Ownership section).
pub struct SubscriptionBook<F: FieldMask, const N: usize> {
    entries: heapless::Vec<(ObservationSubscription<F>, DeliveryTracking), N>,
}

impl<F: FieldMask, const N: usize> SubscriptionBook<F, N> {
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }

    pub fn get(&self, key: SubscriptionKey) -> Option<&ObservationSubscription<F>> {
        self.entries
            .iter()
            .find(|(sub, _)| sub.key == key)
            .map(|(sub, _)| sub)
    }

    pub fn tracking_mut(&mut self, key: SubscriptionKey) -> Option<&mut DeliveryTracking> {
        self.entries
            .iter_mut()
            .find(|(sub, _)| sub.key == key)
            .map(|(_, tracking)| tracking)
    }

    /// Installs `subscription`, replacing any existing entry with the same
    /// key and resetting its delivery tracking. Fails only when the key is
    /// new and the book is already at capacity.
    pub fn upsert(&mut self, subscription: ObservationSubscription<F>) -> Result<(), BookFull> {
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|(sub, _)| sub.key == subscription.key)
        {
            *slot = (subscription, DeliveryTracking::default());
            return Ok(());
        }
        self.entries
            .push((subscription, DeliveryTracking::default()))
            .map_err(|_| BookFull)
    }

    /// Removes the single subscription at `key`. Returns whether one was
    /// present.
    pub fn remove(&mut self, key: SubscriptionKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(sub, _)| sub.key != key);
        self.entries.len() != before
    }

    /// Removes every subscription owned by `owner`, across every generation
    /// and slot -- surface teardown. Returns the count removed.
    pub fn remove_owner(&mut self, owner: OwnerId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(sub, _)| sub.key.owner != owner);
        before - self.entries.len()
    }

    /// Removes every subscription against `provider` -- provider removal
    /// (e.g. a target-owned provider is torn down). Returns the count
    /// removed.
    pub fn remove_provider(&mut self, provider: ProviderId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(sub, _)| sub.key.provider != provider);
        before - self.entries.len()
    }

    /// Drops subscriptions whose `expires_at` has passed. Returns the count
    /// removed.
    pub fn purge_expired(&mut self, now: Instant) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|(sub, _)| !sub.expires_at.is_some_and(|expiry| now >= expiry));
        before - self.entries.len()
    }

    pub fn iter_provider(
        &self,
        provider: ProviderId,
    ) -> impl Iterator<Item = &ObservationSubscription<F>> {
        self.entries
            .iter()
            .filter(move |(sub, _)| sub.key.provider == provider)
            .map(|(sub, _)| sub)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ObservationSubscription<F>> {
        self.entries.iter().map(|(sub, _)| sub)
    }
}

impl<F: FieldMask, const N: usize> Default for SubscriptionBook<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;

    fn key(owner: u16, slot: u8) -> SubscriptionKey {
        SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(owner),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(slot),
        }
    }

    fn sub(owner: u16, slot: u8, fields: TestFields) -> ObservationSubscription<TestFields> {
        ObservationSubscription {
            key: key(owner, slot),
            fields,
            max_age: Duration::from_secs(60),
            initial_policy: InitialPolicy::UseCache,
            min_delivery_interval: Duration::from_secs(60),
            expires_at: None,
        }
    }

    #[test]
    fn upsert_replaces_same_key_and_resets_tracking() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, 0, TestFields::TEMPERATURE)).unwrap();
        book.tracking_mut(key(1, 0)).unwrap().bypass_used = true;

        book.upsert(sub(1, 0, TestFields::HUMIDITY)).unwrap();
        assert_eq!(book.len(), 1);
        assert_eq!(book.get(key(1, 0)).unwrap().fields, TestFields::HUMIDITY);
        assert!(!book.tracking_mut(key(1, 0)).unwrap().bypass_used);
    }

    #[test]
    fn full_book_rejects_new_key_but_allows_replacement() {
        let mut book: SubscriptionBook<TestFields, 2> = SubscriptionBook::new();
        book.upsert(sub(1, 0, TestFields::TEMPERATURE)).unwrap();
        book.upsert(sub(2, 0, TestFields::TEMPERATURE)).unwrap();
        assert!(book.is_full());

        assert_eq!(
            book.upsert(sub(3, 0, TestFields::TEMPERATURE)),
            Err(BookFull)
        );
        // Replacing an existing key still succeeds even when full.
        assert!(book.upsert(sub(1, 0, TestFields::HUMIDITY)).is_ok());
    }

    #[test]
    fn remove_owner_drops_every_slot_and_generation() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, 0, TestFields::TEMPERATURE)).unwrap();
        book.upsert(sub(1, 1, TestFields::HUMIDITY)).unwrap();
        book.upsert(sub(2, 0, TestFields::TEMPERATURE)).unwrap();

        assert_eq!(book.remove_owner(OwnerId(1)), 2);
        assert_eq!(book.len(), 1);
        assert!(book.get(key(2, 0)).is_some());
    }

    #[test]
    fn purge_expired_drops_only_past_expiry() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        let mut expiring = sub(1, 0, TestFields::TEMPERATURE);
        expiring.expires_at = Some(Instant(100));
        book.upsert(expiring).unwrap();
        book.upsert(sub(2, 0, TestFields::TEMPERATURE)).unwrap(); // never expires

        assert_eq!(book.purge_expired(Instant(50)), 0);
        assert_eq!(book.purge_expired(Instant(100)), 1);
        assert_eq!(book.len(), 1);
        assert!(book.get(key(2, 0)).is_some());
    }

    #[test]
    fn stale_owner_generation_is_a_distinct_key() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        let mut original = sub(1, 0, TestFields::TEMPERATURE);
        original.key.owner_generation = OwnerGeneration(0);
        book.upsert(original).unwrap();

        // A recommit under a new generation does not implicitly replace the
        // old one -- the owner must remove it explicitly (e.g. via
        // `remove_owner` during rollback/recovery).
        let mut recommitted = sub(1, 0, TestFields::TEMPERATURE);
        recommitted.key.owner_generation = OwnerGeneration(1);
        book.upsert(recommitted).unwrap();
        assert_eq!(book.len(), 2);

        assert_eq!(book.remove_owner(OwnerId(1)), 2);
    }
}
