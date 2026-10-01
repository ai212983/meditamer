//! A provider's latest observed state (ADR-0018's `ObservationState<S>`
//! envelope): provider identity, generation, revision, last-attempt time,
//! typed health, and a provider-defined snapshot, plus the per-field
//! freshness this crate's scheduling and delivery decisions need.

use crate::field::FieldMask;
use crate::ids::{ProviderGeneration, ProviderId, Revision, RevisionExhausted};
use crate::policy::Health;
use crate::schedule::FieldTimestamps;
use crate::time::Instant;

/// The provider's latest snapshot plus the bookkeeping every provider shares
/// regardless of its own field/snapshot types. `FIELDS` is the caller's
/// declared field-bit capacity, matching [`crate::demand::Demand`]'s.
pub struct ObservationState<S, const FIELDS: usize> {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub last_attempt_at: Option<Instant>,
    pub health: Health,
    pub snapshot: S,
    pub field_timestamps: FieldTimestamps<FIELDS>,
}

impl<S, const FIELDS: usize> ObservationState<S, FIELDS> {
    pub fn new(provider: ProviderId, generation: ProviderGeneration, snapshot: S) -> Self {
        Self {
            provider,
            generation,
            revision: Revision::INITIAL,
            last_attempt_at: None,
            health: Health::Unknown,
            snapshot,
            field_timestamps: FieldTimestamps::new(),
        }
    }

    /// Records a successful acquisition of `fields` at `at`: stamps those
    /// fields, lets the caller merge the new values into `snapshot` (a
    /// combined conversion may merge more than `fields` alone), advances
    /// the revision, and sets health to `Ok`. Fields not in `fields` retain
    /// their existing stamp untouched. Exhaustion leaves the entire state
    /// unchanged and does not invoke `merge`.
    pub fn apply_success<F: FieldMask>(
        &mut self,
        fields: F,
        at: Instant,
        merge: impl FnOnce(&mut S),
    ) -> Result<(), RevisionExhausted> {
        let revision = self.revision.checked_advance()?;
        merge(&mut self.snapshot);
        self.field_timestamps.stamp(fields, at);
        self.last_attempt_at = Some(at);
        self.revision = revision;
        self.health = Health::Ok;
        Ok(())
    }

    /// Records a failed attempt at `at`: advances the revision (a failure is
    /// a completed attempt) and sets health, without touching any field's
    /// stamp -- prior successful fields remain valid cache. Exhaustion leaves
    /// the entire state unchanged.
    pub fn apply_failure(&mut self, at: Instant) -> Result<(), RevisionExhausted> {
        let revision = self.revision.checked_advance()?;
        self.last_attempt_at = Some(at);
        self.revision = revision;
        self.health = if self.field_timestamps.has_any() {
            Health::Degraded
        } else {
            Health::Failed
        };
        Ok(())
    }

    /// Resets generation-scoped bookkeeping on a provider restart. The
    /// snapshot value itself is left to the caller (typically reset to a
    /// provider-defined default alongside this call), since this crate does
    /// not know how to construct one generically.
    pub fn restart(&mut self, generation: ProviderGeneration) {
        self.generation = generation;
        self.revision = Revision::INITIAL;
        self.last_attempt_at = None;
        self.health = Health::Unknown;
        self.field_timestamps = FieldTimestamps::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;

    #[test]
    fn success_stamps_fields_and_advances_revision() {
        let mut state: ObservationState<i32, 4> =
            ObservationState::new(ProviderId(1), ProviderGeneration::INITIAL, 0i32);
        state
            .apply_success(TestFields::TEMPERATURE, Instant(10), |snapshot| {
                *snapshot = 21
            })
            .unwrap();

        assert_eq!(state.snapshot, 21);
        assert_eq!(state.revision, Revision(1));
        assert_eq!(state.health, Health::Ok);
        assert_eq!(state.field_timestamps.get(0), Some(Instant(10)));
    }

    #[test]
    fn failure_retains_prior_field_stamp_and_degrades() {
        let mut state: ObservationState<i32, 4> =
            ObservationState::new(ProviderId(1), ProviderGeneration::INITIAL, 0i32);
        state
            .apply_success(TestFields::TEMPERATURE, Instant(10), |s| *s = 21)
            .unwrap();

        state.apply_failure(Instant(20)).unwrap();
        assert_eq!(state.health, Health::Degraded);
        assert_eq!(state.revision, Revision(2));
        // Prior successful field stamp is untouched by the failure.
        assert_eq!(state.field_timestamps.get(0), Some(Instant(10)));
    }

    #[test]
    fn failure_with_no_prior_success_is_failed_not_degraded() {
        let mut state: ObservationState<i32, 4> =
            ObservationState::new(ProviderId(1), ProviderGeneration::INITIAL, 0i32);
        state.apply_failure(Instant(5)).unwrap();
        assert_eq!(state.health, Health::Failed);
    }

    #[test]
    fn revision_exhaustion_preserves_state_for_success_and_failure() {
        let mut state: ObservationState<i32, 4> =
            ObservationState::new(ProviderId(1), ProviderGeneration::INITIAL, 0i32);
        state
            .apply_success(TestFields::TEMPERATURE, Instant(10), |s| *s = 21)
            .unwrap();
        state.apply_failure(Instant(20)).unwrap();
        state.revision = Revision(u32::MAX);
        assert_eq!(
            state.apply_success(TestFields::TEMPERATURE, Instant(30), |_| {
                panic!("exhausted revision must not merge a new sample")
            }),
            Err(RevisionExhausted)
        );
        assert_eq!(state.apply_failure(Instant(40)), Err(RevisionExhausted));
        assert_eq!(state.snapshot, 21);
        assert_eq!(state.field_timestamps.get(0), Some(Instant(10)));
        assert_eq!(state.field_timestamps.get(1), None);
        assert_eq!(state.last_attempt_at, Some(Instant(20)));
        assert_eq!(state.health, Health::Degraded);
        assert_eq!(state.revision, Revision(u32::MAX));
    }

    #[test]
    fn restart_resets_generation_scoped_bookkeeping() {
        let mut state: ObservationState<i32, 4> =
            ObservationState::new(ProviderId(1), ProviderGeneration::INITIAL, 0i32);
        state
            .apply_success(TestFields::TEMPERATURE, Instant(10), |s| *s = 21)
            .unwrap();

        state.restart(ProviderGeneration::INITIAL.checked_advance().unwrap());
        assert_eq!(state.generation, ProviderGeneration(1));
        assert_eq!(state.revision, Revision::INITIAL);
        assert_eq!(state.health, Health::Unknown);
        assert_eq!(state.field_timestamps.get(0), None);
    }
}
