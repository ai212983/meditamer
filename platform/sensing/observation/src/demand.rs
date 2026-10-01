//! Aggregating a provider's active subscriptions into one demand: the union
//! of requested fields, and the strictest max age per field (ADR-0018:
//! "Aggregate requested fields and the strictest maximum age per field...
//! Only requested fields drive future sampling").

use crate::field::{field_indices, FieldMask};
use crate::ids::ProviderId;
use crate::subscription::SubscriptionBook;
use crate::time::{Duration, Instant};

/// The union of fields a provider must keep fresh, and per-field strictest
/// max age across every subscription currently requesting that field.
///
/// `FIELDS` is the caller's declared field-bit capacity -- the target that
/// owns this provider's storage picks it to match the field type's real bit
/// count (see [`crate::field::SUGGESTED_MAX_FIELDS`]). A field bit at or
/// beyond `FIELDS` is a caller bug (a `FieldMask` impl exposing more bits
/// than the storage it's paired with); [`Self::union_field`] validates
/// against that capacity rather than indexing out of bounds.
#[derive(Clone, Copy)]
pub struct Demand<F: FieldMask, const FIELDS: usize> {
    pub fields: F,
    field_max_age: [Option<Duration>; FIELDS],
}

impl<F: FieldMask, const FIELDS: usize> Demand<F, FIELDS> {
    pub const fn empty() -> Self {
        Demand {
            fields: F::EMPTY,
            field_max_age: [None; FIELDS],
        }
    }

    /// One bounded consumer requesting the same age for each selected field.
    pub fn uniform(fields: F, max_age: Duration) -> Self {
        let mut demand = Self::empty();
        demand.union_field(fields, max_age);
        demand
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The strictest max age requested for the field at bit `index`, or
    /// `None` if nothing currently requests that field, or if `index` is
    /// beyond this demand's declared capacity.
    pub fn max_age_at(&self, index: usize) -> Option<Duration> {
        self.field_max_age.get(index).copied().flatten()
    }

    /// Folds one subscription's fields and max age into this demand. A field
    /// bit at or beyond `FIELDS` is dropped from `fields` too, so `fields`
    /// never claims a field this demand has no timestamp slot for.
    fn union_field(&mut self, fields: F, max_age: Duration) {
        let mut accepted_bits = 0u32;
        for index in field_indices(fields.bits()) {
            let Some(slot) = self.field_max_age.get_mut(index) else {
                continue;
            };
            accepted_bits |= 1 << index;
            *slot = Some(match *slot {
                Some(strictest) if strictest.0 <= max_age.0 => strictest,
                _ => max_age,
            });
        }
        self.fields = self.fields.union(F::from_bits_truncate(accepted_bits));
    }
}

/// Aggregates every non-expired subscription against `provider` in `book`
/// into one [`Demand`].
pub fn aggregate_demand<F: FieldMask, const N: usize, const FIELDS: usize>(
    book: &SubscriptionBook<F, N>,
    provider: ProviderId,
    now: Instant,
) -> Demand<F, FIELDS> {
    let mut demand: Demand<F, FIELDS> = Demand::empty();
    for sub in book.iter_provider(provider) {
        if sub.expires_at.is_some_and(|expiry| now >= expiry) {
            continue;
        }
        demand.union_field(sub.fields, sub.max_age);
    }
    demand
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;
    use crate::ids::{OwnerGeneration, OwnerId, SubscriptionSlot};
    use crate::policy::InitialPolicy;
    use crate::subscription::{ObservationSubscription, SubscriptionKey};

    fn sub(
        owner: u16,
        fields: TestFields,
        max_age: Duration,
    ) -> ObservationSubscription<TestFields> {
        ObservationSubscription {
            key: SubscriptionKey {
                provider: ProviderId(1),
                owner: OwnerId(owner),
                owner_generation: OwnerGeneration(0),
                slot: SubscriptionSlot(0),
            },
            fields,
            max_age,
            initial_policy: InitialPolicy::UseCache,
            min_delivery_interval: max_age,
            expires_at: None,
        }
    }

    #[test]
    fn empty_book_yields_empty_demand() {
        let book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
        assert!(demand.is_empty());
    }

    #[test]
    fn union_of_fields_and_strictest_max_age_per_field() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        // Five-minute temperature consumer beside a one-minute
        // temperature+humidity consumer (validation matrix: Core scheduling).
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(300)))
            .unwrap();
        book.upsert(sub(
            2,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Duration::from_secs(60),
        ))
        .unwrap();

        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
        assert_eq!(
            demand.fields,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY)
        );
        assert_eq!(demand.max_age_at(0), Some(Duration::from_secs(60))); // temperature
        assert_eq!(demand.max_age_at(1), Some(Duration::from_secs(60))); // humidity
    }

    #[test]
    fn removing_the_faster_consumer_relaxes_max_age() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(300)))
            .unwrap();
        book.upsert(sub(2, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();
        book.remove(SubscriptionKey {
            provider: ProviderId(1),
            owner: OwnerId(2),
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        });

        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
        assert_eq!(demand.max_age_at(0), Some(Duration::from_secs(300)));
    }

    #[test]
    fn independent_provider_ids_do_not_mix_demand() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        let mut other_provider = sub(1, TestFields::HUMIDITY, Duration::from_secs(10));
        other_provider.key.provider = ProviderId(2);
        book.upsert(other_provider).unwrap();
        book.upsert(sub(2, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();

        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
        assert_eq!(demand.fields, TestFields::TEMPERATURE);
        assert_eq!(demand.max_age_at(1), None);
    }

    #[test]
    fn expired_subscription_does_not_contribute_demand() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        let mut expiring = sub(1, TestFields::TEMPERATURE, Duration::from_secs(60));
        expiring.expires_at = Some(Instant(100));
        book.upsert(expiring).unwrap();

        assert!(
            !aggregate_demand::<TestFields, 4, 4>(&book, ProviderId(1), Instant(50)).is_empty()
        );
        assert!(
            aggregate_demand::<TestFields, 4, 4>(&book, ProviderId(1), Instant(100)).is_empty()
        );
    }

    #[test]
    fn a_field_bit_beyond_capacity_is_dropped_not_out_of_bounds() {
        // A `FIELDS` capacity smaller than a field bit some subscription
        // requests is a caller bug (declared capacity narrower than the
        // field type's own bits), not a panic: the out-of-range bit is
        // dropped from both the timestamp slot and `demand.fields` itself.
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(
            1,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Duration::from_secs(60),
        ))
        .unwrap();

        let demand: Demand<TestFields, 1> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);
        assert_eq!(demand.fields, TestFields::TEMPERATURE); // humidity (index 1) dropped
        assert_eq!(demand.max_age_at(0), Some(Duration::from_secs(60)));
        assert_eq!(demand.max_age_at(1), None);
    }
}
