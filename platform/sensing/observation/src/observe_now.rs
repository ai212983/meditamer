//! Observe-now: an expiring one-shot acquisition request alongside
//! persistent demand (ADR-0018). Results are delivered through latest state
//! and health, not individual reply messages; this module only tracks
//! admission, expiry, and which admitted requests a completed conversion
//! satisfies.

use crate::field::FieldMask;
use crate::ids::{OwnerGeneration, OwnerId};
use crate::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserveNowRequest<F: FieldMask> {
    pub owner: OwnerId,
    pub owner_generation: OwnerGeneration,
    pub fields: F,
    pub admitted_at: Instant,
    pub expires_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

/// A fixed-capacity queue of admitted observe-now requests for one provider.
pub struct ObserveNowQueue<F: FieldMask, const N: usize> {
    entries: heapless::Vec<ObserveNowRequest<F>, N>,
}

impl<F: FieldMask, const N: usize> ObserveNowQueue<F, N> {
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

    /// Admits `request`, rejecting it explicitly if the queue is at
    /// capacity ("explicit queue admission or rejection").
    pub fn admit(&mut self, request: ObserveNowRequest<F>) -> Result<(), QueueFull> {
        self.entries.push(request).map_err(|_| QueueFull)
    }

    /// Drops every request whose `expires_at` has passed. Returns the count
    /// removed.
    pub fn purge_expired(&mut self, now: Instant) -> usize {
        let before = self.entries.len();
        self.entries.retain(|req| req.expires_at > now);
        before - self.entries.len()
    }

    /// The next expiry the provider must wake for even while acquisition is
    /// rate-limited or backing off, so transport capacity can be returned.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries.iter().map(|request| request.expires_at).min()
    }

    /// The union of every admitted request's fields, regardless of which
    /// conversion will end up satisfying each -- used to fold pending
    /// one-shots into what the next acquisition should measure.
    pub fn pending_fields(&self) -> F {
        self.entries
            .iter()
            .fold(F::EMPTY, |acc, req| acc.union(req.fields))
    }

    /// Drops every request from `owner`, regardless of generation --
    /// removing an owner discards its pending one-shots.
    pub fn remove_owner(&mut self, owner: OwnerId) -> usize {
        let before = self.entries.len();
        self.entries.retain(|req| req.owner != owner);
        before - self.entries.len()
    }

    /// Removes and returns every admitted request that `fields_measured`
    /// satisfies (its requested fields are a subset of what was measured)
    /// and that was admitted no later than `completed_at`. Compatible
    /// requests admitted during conversion can use its result. Requests
    /// expiring at or before completion are discarded without satisfaction.
    pub fn take_satisfied(
        &mut self,
        fields_measured: F,
        completed_at: Instant,
    ) -> heapless::Vec<ObserveNowRequest<F>, N> {
        let mut satisfied = heapless::Vec::new();
        let mut remaining = heapless::Vec::new();
        for req in self.entries.drain(..) {
            if req.expires_at <= completed_at {
                continue;
            }
            if req.admitted_at <= completed_at && fields_measured.contains(req.fields) {
                // Both are bounded by the same capacity N, so these pushes
                // cannot fail.
                let _ = satisfied.push(req);
            } else {
                let _ = remaining.push(req);
            }
        }
        self.entries = remaining;
        satisfied
    }
}

impl<F: FieldMask, const N: usize> Default for ObserveNowQueue<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;

    fn req(
        owner: u16,
        fields: TestFields,
        admitted_at: u64,
        expires_at: u64,
    ) -> ObserveNowRequest<TestFields> {
        ObserveNowRequest {
            owner: OwnerId(owner),
            owner_generation: OwnerGeneration(0),
            fields,
            admitted_at: Instant(admitted_at),
            expires_at: Instant(expires_at),
        }
    }

    #[test]
    fn full_queue_rejects_admission_explicitly() {
        let mut queue: ObserveNowQueue<TestFields, 1> = ObserveNowQueue::new();
        queue
            .admit(req(1, TestFields::TEMPERATURE, 0, 100))
            .unwrap();
        assert_eq!(
            queue.admit(req(2, TestFields::TEMPERATURE, 0, 100)),
            Err(QueueFull)
        );
    }

    #[test]
    fn purge_expired_drops_only_past_expiry() {
        let mut queue: ObserveNowQueue<TestFields, 4> = ObserveNowQueue::new();
        queue
            .admit(req(1, TestFields::TEMPERATURE, 0, 100))
            .unwrap();
        queue
            .admit(req(2, TestFields::TEMPERATURE, 0, 1_000))
            .unwrap();

        assert_eq!(queue.purge_expired(Instant(100)), 1);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn conversion_satisfies_requests_admitted_before_it_completed() {
        let mut queue: ObserveNowQueue<TestFields, 4> = ObserveNowQueue::new();
        queue
            .admit(req(1, TestFields::TEMPERATURE, 0, 1_000))
            .unwrap();
        // A compatible request admitted while conversion is in progress.
        queue
            .admit(req(2, TestFields::TEMPERATURE, 50, 1_000))
            .unwrap();

        let satisfied = queue.take_satisfied(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(100),
        );
        assert_eq!(satisfied.len(), 2);
        assert_eq!(satisfied[0].owner, OwnerId(1));
        assert_eq!(satisfied[1].owner, OwnerId(2));
        assert!(queue.is_empty());
    }

    #[test]
    fn conversion_only_satisfies_requests_whose_fields_it_measured() {
        let mut queue: ObserveNowQueue<TestFields, 4> = ObserveNowQueue::new();
        queue
            .admit(req(
                1,
                TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
                0,
                1_000,
            ))
            .unwrap();

        let satisfied = queue.take_satisfied(TestFields::TEMPERATURE, Instant(10));
        assert!(satisfied.is_empty());
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn completion_discards_expired_and_keeps_later_admissions() {
        let mut queue: ObserveNowQueue<TestFields, 3> = ObserveNowQueue::new();
        queue
            .admit(req(1, TestFields::TEMPERATURE, 0, 100))
            .unwrap();
        queue
            .admit(req(2, TestFields::TEMPERATURE, 101, 1_000))
            .unwrap();
        queue
            .admit(req(3, TestFields::TEMPERATURE, 50, 1_000))
            .unwrap();

        let satisfied = queue.take_satisfied(TestFields::TEMPERATURE, Instant(100));
        assert_eq!(satisfied.len(), 1);
        assert_eq!(satisfied[0].owner, OwnerId(3));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.entries[0].owner, OwnerId(2));
    }

    #[test]
    fn remove_owner_discards_pending_one_shots() {
        let mut queue: ObserveNowQueue<TestFields, 4> = ObserveNowQueue::new();
        queue
            .admit(req(1, TestFields::TEMPERATURE, 0, 1_000))
            .unwrap();
        queue
            .admit(req(2, TestFields::TEMPERATURE, 0, 1_000))
            .unwrap();

        assert_eq!(queue.remove_owner(OwnerId(1)), 1);
        assert_eq!(queue.len(), 1);
    }
}
