//! Policy foundation for transactional asset residency.
//!
//! Identity newtypes, generation-checked handles, the prepare/commit
//! participant contract, and the coordinator for exactly two heterogeneous
//! participants.
//!
//! A participant stages one request through [`Participant::prepare`] into an
//! owned [`Provisional`]. Dropping an uncommitted provisional rolls its staged
//! work back. [`Provisional::commit`] takes the staged value out, so the
//! [`Drop`] implementation that runs afterwards (Rust always runs `Drop` when
//! the consuming `self` goes out of scope at the end of `commit`) finds no
//! staged value and performs no rollback. [`coordinate`] prepares left then
//! right, drops (rolls back) left when right prepare fails, and commits both
//! only after both prepare successfully.

#![no_std]

mod bundle;

pub use bundle::{Bundle, BundleError, BundleId, BundleLayout, BundlePart};

/// Identifies a managed resource (an asset slot, a block range owner).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResourceId(pub u16);

/// Identifies one load or activation request for a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u32);

/// Activation generation a handle or completion belongs to. A completion for
/// a stale generation must be rejected, never applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Generation(pub u32);

/// Generation-checked handle to a resource: the resource, the request that
/// produced this handle, and the generation it is valid for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ResourceHandle {
    resource: ResourceId,
    request: RequestId,
    generation: Generation,
}

impl ResourceHandle {
    /// Name a resource as produced by `request` at `generation`.
    pub const fn new(resource: ResourceId, request: RequestId, generation: Generation) -> Self {
        Self {
            resource,
            request,
            generation,
        }
    }

    /// The named resource.
    pub const fn resource(self) -> ResourceId {
        self.resource
    }

    /// The request that produced this handle.
    pub const fn request(self) -> RequestId {
        self.request
    }

    /// The generation this handle is valid for.
    pub const fn generation(self) -> Generation {
        self.generation
    }

    /// True only when every identity matches.
    pub const fn matches_exact(self, other: Self) -> bool {
        self.resource.0 == other.resource.0
            && self.request.0 == other.request.0
            && self.generation.0 == other.generation.0
    }

    /// True when this handle names `resource` at `generation`, whatever
    /// request produced it. Stale-generation completions fail this check.
    pub const fn is_current_for(self, resource: ResourceId, generation: Generation) -> bool {
        self.resource.0 == resource.0 && self.generation.0 == generation.0
    }
}

/// Mechanism-owned staging for one prepared request.
///
/// Commit publishes the staged work and yields the mechanism's committed
/// output. Rollback discards the staged work. Both consume the staged value
/// and are infallible by contract: anything that can fail stays in
/// [`Participant::prepare`].
pub trait Staged {
    /// Committed output published by `commit`. Each mechanism names its own.
    type Output;
    /// Publish the staged work.
    fn commit(self) -> Self::Output;
    /// Discard the staged work.
    fn rollback(self);
}

/// Policy-owned provisional wrapping one staged value.
///
/// Holds the staged value in a private [`Option`]. [`Provisional::commit`]
/// takes the value and commits it; [`Drop`] takes the value and rolls it back
/// only when a staged value is still present (that is, when `commit` never
/// ran). `Drop` still runs after `commit` consumes `self`, but finds the slot
/// empty and does nothing.
pub struct Provisional<S: Staged> {
    staged: Option<S>,
}

impl<S: Staged> Provisional<S> {
    /// Wrap a staged value in policy ownership.
    pub fn new(staged: S) -> Self {
        Self {
            staged: Some(staged),
        }
    }

    /// Commit the staged value, yielding its output.
    pub fn commit(mut self) -> S::Output {
        self.staged
            .take()
            .expect("provisional commit has staged value")
            .commit()
    }
}

impl<S: Staged> Drop for Provisional<S> {
    fn drop(&mut self) {
        if let Some(staged) = self.staged.take() {
            staged.rollback();
        }
    }
}

/// A mechanism that can provisionally stage one request.
///
/// Each mechanism owns only its local staged state; the joint
/// two-participant transaction belongs to [`coordinate`].
pub trait Participant {
    /// Mechanism-owned staging for one prepared request.
    type Staged: Staged;
    /// Why prepare failed. Left/right identity survives in [`PairError`].
    type Error;

    /// Stage `handle`, returning the policy-owned provisional on success.
    fn prepare(&mut self, handle: ResourceHandle)
        -> Result<Provisional<Self::Staged>, Self::Error>;
}

/// Which side of a two-participant transaction failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairError<L, R> {
    /// The left participant's prepare failed; right was never prepared.
    Left(L),
    /// The right participant's prepare failed; left was rolled back.
    Right(R),
}

/// Run the two-participant transaction for `handle`: prepare left, then
/// right, then commit both, returning the heterogeneous pair of committed
/// outputs. If right prepare fails, the left provisional is dropped (rolled
/// back) and the right error is returned.
#[allow(clippy::type_complexity)]
pub fn coordinate<L, R>(
    left: &mut L,
    right: &mut R,
    handle: ResourceHandle,
) -> Result<
    (
        <<L as Participant>::Staged as Staged>::Output,
        <<R as Participant>::Staged as Staged>::Output,
    ),
    PairError<L::Error, R::Error>,
>
where
    L: Participant,
    R: Participant,
{
    let staged_left = left.prepare(handle).map_err(PairError::Left)?;
    let staged_right = match right.prepare(handle) {
        Ok(staged) => staged,
        Err(error) => {
            drop(staged_left);
            return Err(PairError::Right(error));
        }
    };
    let left_output = staged_left.commit();
    let right_output = staged_right.commit();
    Ok((left_output, right_output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct LeftError;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct RightError;

    struct Counters {
        prepares: Cell<u32>,
        commits: Cell<u32>,
        rollbacks: Cell<u32>,
    }

    impl Counters {
        const fn new() -> Self {
            Self {
                prepares: Cell::new(0),
                commits: Cell::new(0),
                rollbacks: Cell::new(0),
            }
        }
    }

    struct LeftStaged<'a> {
        counters: &'a Counters,
    }

    struct RightStaged<'a> {
        counters: &'a Counters,
    }

    impl Staged for LeftStaged<'_> {
        type Output = u16;

        fn commit(self) -> u16 {
            self.counters.commits.set(self.counters.commits.get() + 1);
            0x2au16
        }

        fn rollback(self) {
            self.counters
                .rollbacks
                .set(self.counters.rollbacks.get() + 1);
        }
    }

    impl Staged for RightStaged<'_> {
        type Output = u32;

        fn commit(self) -> u32 {
            self.counters.commits.set(self.counters.commits.get() + 1);
            0x2b00u32
        }

        fn rollback(self) {
            self.counters
                .rollbacks
                .set(self.counters.rollbacks.get() + 1);
        }
    }

    struct LeftParticipant<'a> {
        counters: &'a Counters,
        fail: bool,
    }

    struct RightParticipant<'a> {
        counters: &'a Counters,
        fail: bool,
    }

    impl<'a> Participant for LeftParticipant<'a> {
        type Staged = LeftStaged<'a>;
        type Error = LeftError;

        fn prepare(
            &mut self,
            _handle: ResourceHandle,
        ) -> Result<Provisional<Self::Staged>, Self::Error> {
            self.counters.prepares.set(self.counters.prepares.get() + 1);
            if self.fail {
                return Err(LeftError);
            }
            Ok(Provisional::new(LeftStaged {
                counters: self.counters,
            }))
        }
    }

    impl<'a> Participant for RightParticipant<'a> {
        type Staged = RightStaged<'a>;
        type Error = RightError;

        fn prepare(
            &mut self,
            _handle: ResourceHandle,
        ) -> Result<Provisional<Self::Staged>, Self::Error> {
            self.counters.prepares.set(self.counters.prepares.get() + 1);
            if self.fail {
                return Err(RightError);
            }
            Ok(Provisional::new(RightStaged {
                counters: self.counters,
            }))
        }
    }

    const fn handle() -> ResourceHandle {
        ResourceHandle::new(ResourceId(3), RequestId(11), Generation(7))
    }

    #[test]
    fn left_failure_touches_neither_provisional() {
        let left_counters = Counters::new();
        let right_counters = Counters::new();
        let mut left = LeftParticipant {
            counters: &left_counters,
            fail: true,
        };
        let mut right = RightParticipant {
            counters: &right_counters,
            fail: false,
        };
        assert_eq!(
            coordinate(&mut left, &mut right, handle()),
            Err(PairError::Left(LeftError))
        );
        assert_eq!(left_counters.prepares.get(), 1);
        assert_eq!(right_counters.prepares.get(), 0);
        assert_eq!(left_counters.commits.get(), 0);
        assert_eq!(right_counters.commits.get(), 0);
        assert_eq!(left_counters.rollbacks.get(), 0);
        assert_eq!(right_counters.rollbacks.get(), 0);
    }

    #[test]
    fn right_failure_rolls_back_left_exactly_once() {
        let left_counters = Counters::new();
        let right_counters = Counters::new();
        let mut left = LeftParticipant {
            counters: &left_counters,
            fail: false,
        };
        let mut right = RightParticipant {
            counters: &right_counters,
            fail: true,
        };
        assert_eq!(
            coordinate(&mut left, &mut right, handle()),
            Err(PairError::Right(RightError))
        );
        assert_eq!(left_counters.prepares.get(), 1);
        assert_eq!(right_counters.prepares.get(), 1);
        assert_eq!(left_counters.commits.get(), 0);
        assert_eq!(right_counters.commits.get(), 0);
        assert_eq!(left_counters.rollbacks.get(), 1);
        assert_eq!(right_counters.rollbacks.get(), 0);
    }

    #[test]
    fn success_commits_both_and_rolls_back_neither() {
        let left_counters = Counters::new();
        let right_counters = Counters::new();
        let mut left = LeftParticipant {
            counters: &left_counters,
            fail: false,
        };
        let mut right = RightParticipant {
            counters: &right_counters,
            fail: false,
        };
        assert_eq!(
            coordinate(&mut left, &mut right, handle()),
            Ok((0x2au16, 0x2b00u32))
        );
        assert_eq!(left_counters.prepares.get(), 1);
        assert_eq!(right_counters.prepares.get(), 1);
        assert_eq!(left_counters.commits.get(), 1);
        assert_eq!(right_counters.commits.get(), 1);
        assert_eq!(left_counters.rollbacks.get(), 0);
        assert_eq!(right_counters.rollbacks.get(), 0);
    }

    #[test]
    fn dropped_standalone_provisional_rolls_back() {
        let counters = Counters::new();
        let mut participant = LeftParticipant {
            counters: &counters,
            fail: false,
        };
        let provisional = participant.prepare(handle()).expect("prepare succeeds");
        drop(provisional);
        assert_eq!(counters.commits.get(), 0);
        assert_eq!(counters.rollbacks.get(), 1);
    }

    #[test]
    fn committed_provisional_does_not_roll_back() {
        let counters = Counters::new();
        let mut participant = LeftParticipant {
            counters: &counters,
            fail: false,
        };
        let provisional = participant.prepare(handle()).expect("prepare succeeds");
        assert_eq!(provisional.commit(), 0x2au16);
        assert_eq!(counters.commits.get(), 1);
        assert_eq!(counters.rollbacks.get(), 0);
    }

    #[test]
    fn exact_match_rejects_any_mismatch() {
        let base = handle();
        assert!(base.matches_exact(base));
        assert!(!base.matches_exact(ResourceHandle::new(
            ResourceId(4),
            RequestId(11),
            Generation(7)
        )));
        assert!(!base.matches_exact(ResourceHandle::new(
            ResourceId(3),
            RequestId(12),
            Generation(7)
        )));
        assert!(!base.matches_exact(ResourceHandle::new(
            ResourceId(3),
            RequestId(11),
            Generation(8)
        )));
    }

    #[test]
    fn current_generation_ignores_request_but_not_resource_or_generation() {
        let base = handle();
        let same_generation_other_request =
            ResourceHandle::new(ResourceId(3), RequestId(99), Generation(7));
        assert!(base.is_current_for(ResourceId(3), Generation(7)));
        assert!(same_generation_other_request.is_current_for(ResourceId(3), Generation(7)));
        assert!(!base.is_current_for(ResourceId(4), Generation(7)));
        assert!(!base.is_current_for(ResourceId(3), Generation(8)));
    }
}
