//! Scheduling acquisition from field timestamps and demand (ADR-0018).
//! `max_age` classifies freshness, not a guaranteed delivery deadline: a
//! field becomes due at last success plus maximum age. Driver conversion
//! time and resource/executor delays can leave a stale interval. Hardware
//! waits stay in drivers; no guessed lead time is subtracted here.

use crate::demand::Demand;
use crate::field::{field_indices, FieldMask};
use crate::time::Instant;

/// Per-field last-successful-acquisition timestamps for one provider.
/// Stamped independently per field so a combined conversion (e.g. SHTC3's
/// temperature+humidity) can update some fields while others retain an
/// older stamp, and so a field's stamp survives a later failed attempt.
///
/// `FIELDS` is the caller's declared field-bit capacity, matching the
/// [`Demand`] it is scheduled against (see [`Demand`]'s doc comment). A field
/// bit at or beyond `FIELDS` is dropped rather than indexed out of bounds.
#[derive(Clone, Copy)]
pub struct FieldTimestamps<const FIELDS: usize>([Option<Instant>; FIELDS]);

impl<const FIELDS: usize> FieldTimestamps<FIELDS> {
    pub const fn new() -> Self {
        Self([None; FIELDS])
    }

    /// Records `at` as the last-success time for every field set in `mask`
    /// that fits within this instance's declared capacity.
    pub fn stamp<F: FieldMask>(&mut self, fields: F, at: Instant) {
        for index in field_indices(fields.bits()) {
            if let Some(slot) = self.0.get_mut(index) {
                *slot = Some(at);
            }
        }
    }

    pub fn get(&self, index: usize) -> Option<Instant> {
        self.0.get(index).copied().flatten()
    }

    /// Whether any field has ever been successfully stamped -- used to
    /// distinguish [`crate::policy::Health::Degraded`] (a usable cache
    /// remains) from [`crate::policy::Health::Failed`] (nothing has ever
    /// succeeded).
    pub fn has_any(&self) -> bool {
        self.0.iter().any(Option::is_some)
    }
}

impl<const FIELDS: usize> Default for FieldTimestamps<FIELDS> {
    fn default() -> Self {
        Self::new()
    }
}

/// What a provider should do next: which fields are due for acquisition
/// right now, and (if any requested field is not yet due) the earliest
/// instant one will become due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcquisitionPlan<F: FieldMask> {
    pub due_now: F,
    pub next_deadline: Option<Instant>,
}

/// Decides `due_now`/`next_deadline` from `demand` and each field's last
/// success in `cache`. A field with no prior success is due immediately.
pub fn plan_acquisition<F: FieldMask, const FIELDS: usize>(
    demand: &Demand<F, FIELDS>,
    cache: &FieldTimestamps<FIELDS>,
    now: Instant,
) -> AcquisitionPlan<F> {
    let mut due_bits = 0u32;
    let mut next_deadline: Option<Instant> = None;

    for index in field_indices(demand.fields.bits()) {
        let max_age = demand.max_age_at(index).unwrap_or_default();
        match cache.get(index) {
            None => due_bits |= 1 << index,
            Some(last_success) => {
                let due_at = last_success.saturating_add(max_age);
                if due_at <= now {
                    due_bits |= 1 << index;
                } else {
                    next_deadline = Some(match next_deadline {
                        Some(earliest) if earliest <= due_at => earliest,
                        _ => due_at,
                    });
                }
            }
        }
    }

    AcquisitionPlan {
        due_now: F::from_bits_truncate(due_bits),
        next_deadline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demand::aggregate_demand;
    use crate::field::test_fields::TestFields;
    use crate::ids::{OwnerGeneration, OwnerId, ProviderId, SubscriptionSlot};
    use crate::policy::InitialPolicy;
    use crate::subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey};
    use crate::time::Duration;

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
    fn missing_field_is_due_immediately() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);

        let plan = plan_acquisition(&demand, &FieldTimestamps::<4>::new(), Instant::ZERO);
        assert_eq!(plan.due_now, TestFields::TEMPERATURE);
        assert_eq!(plan.next_deadline, None);
    }

    #[test]
    fn fresh_field_is_not_due_and_reports_next_deadline() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant(1_000));

        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(TestFields::TEMPERATURE, Instant(1_000));

        let plan = plan_acquisition(&demand, &cache, Instant(1_000));
        assert!(plan.due_now.is_empty());
        assert_eq!(plan.next_deadline, Some(Instant(1_000 + 60_000)));
    }

    #[test]
    fn stale_field_becomes_due_at_its_deadline() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant(61_000));

        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(TestFields::TEMPERATURE, Instant(0));

        let plan = plan_acquisition(&demand, &cache, Instant(61_000));
        assert_eq!(plan.due_now, TestFields::TEMPERATURE);
    }

    #[test]
    fn combined_conversion_covers_both_requested_fields() {
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(
            1,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Duration::from_secs(60),
        ))
        .unwrap();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant::ZERO);

        let plan = plan_acquisition(&demand, &FieldTimestamps::<4>::new(), Instant::ZERO);
        assert_eq!(
            plan.due_now,
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY)
        );
    }

    #[test]
    fn only_requested_fields_drive_the_plan() {
        // Humidity was cached earlier by a combined conversion but nothing
        // currently requests it -- it must not appear due or affect the
        // deadline.
        let mut book: SubscriptionBook<TestFields, 4> = SubscriptionBook::new();
        book.upsert(sub(1, TestFields::TEMPERATURE, Duration::from_secs(60)))
            .unwrap();
        let demand: Demand<TestFields, 4> = aggregate_demand(&book, ProviderId(1), Instant(1_000));

        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(TestFields::TEMPERATURE, Instant(1_000));
        // Humidity has no prior stamp at all -- would be "due" if it were
        // demanded, but it is not, so it must not surface.
        let plan = plan_acquisition(&demand, &cache, Instant(1_000));
        assert!(plan.due_now.is_empty());
    }
}
