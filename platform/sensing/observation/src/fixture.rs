//! Bounded fixture sessions and typed terminal evidence, without target I/O.
//! Products choose provider/owner identities and validity; targets own admission.

pub mod command;
mod session;
use crate::field::FieldMask;
use crate::ids::{OwnerGeneration, OwnerId, ProviderGeneration, ProviderId, Revision};
use crate::policy::Health;
use crate::time::Instant;
use core::fmt;
pub use session::FixtureState;

/// Transient shared metadata; values stay in each provider's typed snapshot.
#[derive(Clone, Copy, Debug)]
pub struct SampleMetadata {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub health: Health,
    pub last_attempt_at: Option<Instant>,
    pub last_sample_at: Option<Instant>,
    pub last_sample_revision: Option<Revision>,
}

impl SampleMetadata {
    pub fn sample_identity(&self) -> Option<(ProviderGeneration, Revision)> {
        self.last_sample_revision
            .map(|revision| (self.generation, revision))
    }
}

pub trait FixtureSample: Copy {
    type Fields: FieldMask;
    const PROVIDER: ProviderId;
    const OWNER: OwnerId;
    const MAX_VALIDITY_MS: u32;
    fn fields() -> Self::Fields;
    fn metadata(self) -> SampleMetadata;
    fn write_values(state: Option<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixtureRequest {
    pub id: u64,
    pub expires_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureStatus {
    Sampled,
    Expired,
    Cancelled,
    Restarted,
    Busy,
    Invalid,
    StaleId,
    GenerationExhausted,
    Full,
    Closed,
    Unavailable,
}

#[derive(Clone, Copy, Debug)]
pub struct FixtureResult<S> {
    pub id: u64,
    pub owner_generation: OwnerGeneration,
    pub admitted_at: Option<Instant>,
    pub expires_at: Instant,
    pub status: FixtureStatus,
    pub state: Option<S>,
    pub baseline: Option<(ProviderGeneration, Revision)>,
}

/// Wire options use `none`, never Debug's `Some(...)`/`None` spelling.
pub struct Optional<T>(pub Option<T>);
impl<T: fmt::Debug> fmt::Display for Optional<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(value) => write!(f, "{value:?}"),
            None => f.write_str("none"),
        }
    }
}

impl<S: FixtureSample> fmt::Display for FixtureResult<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.map(FixtureSample::metadata);
        write!(f, "OBSFIX RESULT id={} provider={} fields={} status={:?} owner_generation={} admitted_at_ms={} expires_at_ms={} generation={} revision={} health={} last_attempt_at_ms={} last_sample_at_ms={} last_sample_revision={}",
            self.id, S::PROVIDER.0, S::fields().bits(), self.status, self.owner_generation.0,
            Optional(self.admitted_at.map(|at| at.0)), self.expires_at.0,
            Optional(state.map(|state| state.generation.0)),
            Optional(state.map(|state| state.revision.0)),
            Optional(state.map(|state| state.health)),
            Optional(state.and_then(|state| state.last_attempt_at.map(|at| at.0))),
            Optional(state.and_then(|state| state.last_sample_at.map(|at| at.0))),
            Optional(state.and_then(|state| state.last_sample_revision.map(|revision| revision.0))),
        )?;
        S::write_values(self.state, f)?;
        write!(
            f,
            " baseline_generation={} baseline_sample_revision={}",
            Optional(self.baseline.map(|(generation, _)| generation.0)),
            Optional(self.baseline.map(|(_, revision)| revision.0))
        )
    }
}
