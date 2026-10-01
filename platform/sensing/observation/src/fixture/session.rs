//! One typed headless observe-now consumer per provider, owned beside persistent delivery.
//! It never changes periodic demand or returns the provider's request credits.

use crate::ids::{OwnerGeneration, ProviderGeneration, Revision};
use crate::ingress::RequestAdmissionError;
use crate::observe_now::ObserveNowRequest;
use crate::policy::Health;
use crate::time::{Duration, Instant};

use super::{FixtureRequest, FixtureResult, FixtureSample, FixtureStatus, SampleMetadata};
use core::marker::PhantomData;

pub struct FixtureState<S> {
    sample: PhantomData<S>,
    generation: OwnerGeneration,
    last_request_id: u64,
    pending: Option<PendingFixture>,
}

struct PendingFixture {
    request: FixtureRequest,
    generation: OwnerGeneration,
    admitted_at: Instant,
    baseline: Option<(ProviderGeneration, Revision)>,
    provider_generation: Option<ProviderGeneration>,
    close_generation: u32,
}

impl PendingFixture {
    fn finish<S>(self, status: FixtureStatus, state: Option<S>) -> FixtureResult<S> {
        FixtureResult {
            id: self.request.id,
            owner_generation: self.generation,
            admitted_at: Some(self.admitted_at),
            expires_at: self.request.expires_at,
            baseline: self.baseline,
            status,
            state,
        }
    }

    fn qualifies(&self, state: &SampleMetadata) -> bool {
        let (Some(at), Some(identity)) = (state.last_sample_at, state.sample_identity()) else {
            return false;
        };
        let advanced = match self.baseline {
            Some((generation, revision)) => identity.0 == generation && identity.1 > revision,
            None => identity.1 > Revision::INITIAL,
        };
        advanced
            && identity.1 <= state.revision
            && matches!(state.health, Health::Ok | Health::Degraded)
            && at >= self.admitted_at
            && at < self.request.expires_at
    }
}

impl<S> Default for FixtureState<S> {
    fn default() -> Self {
        Self {
            sample: PhantomData,
            generation: OwnerGeneration::INITIAL,
            last_request_id: 0,
            pending: None,
        }
    }
}

impl<S: FixtureSample> FixtureState<S> {
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// `None` acknowledges an admitted pending request. Every returned value is
    /// a terminal rejection; the caller emits it once. IDs accepted for handling
    /// are consumed even on rejection, so retries require a new correlation ID.
    pub fn begin(
        &mut self,
        request: FixtureRequest,
        now: Instant,
        state: Option<S>,
        close_generation: u32,
        admit: impl FnOnce(ObserveNowRequest<S::Fields>, Instant) -> Result<(), RequestAdmissionError>,
    ) -> Option<FixtureResult<S>> {
        if request.id == 0 {
            return Some(Self::reject(request, FixtureStatus::Invalid));
        }
        if request.id <= self.last_request_id {
            return Some(Self::reject(request, FixtureStatus::StaleId));
        }
        self.last_request_id = request.id;
        if request.expires_at <= now {
            return Some(Self::reject(request, FixtureStatus::Expired));
        }
        if request.expires_at > now.saturating_add(Duration::from_millis(S::MAX_VALIDITY_MS)) {
            return Some(Self::reject(request, FixtureStatus::Invalid));
        }
        if self.pending.is_some() {
            return Some(Self::reject(request, FixtureStatus::Busy));
        }
        if state.is_some_and(|state| state.metadata().provider != S::PROVIDER) {
            return Some(Self::reject(request, FixtureStatus::Unavailable));
        }
        let Ok(generation) = self.generation.checked_advance() else {
            return Some(Self::reject(request, FixtureStatus::GenerationExhausted));
        };
        let observe = ObserveNowRequest {
            owner: S::OWNER,
            owner_generation: generation,
            fields: S::fields(),
            admitted_at: now,
            expires_at: request.expires_at,
        };
        if let Err(error) = admit(observe, now) {
            let status = match error {
                RequestAdmissionError::Full => FixtureStatus::Full,
                RequestAdmissionError::Closed => FixtureStatus::Closed,
                RequestAdmissionError::Expired => FixtureStatus::Expired,
                RequestAdmissionError::Invalid => FixtureStatus::Invalid,
            };
            return Some(Self::reject(request, status));
        }
        self.generation = generation;
        self.pending = Some(PendingFixture {
            request,
            generation,
            admitted_at: now,
            baseline: state
                .map(FixtureSample::metadata)
                .as_ref()
                .and_then(SampleMetadata::sample_identity),
            provider_generation: state.map(|state| state.metadata().generation),
            close_generation,
        });
        None
    }

    /// Completion validity uses provider time, so an in-window success may be
    /// observed after expiry. Closure cancels first; restart never satisfies a
    /// request made against the previous provider. No terminal result is replayed.
    pub fn poll(
        &mut self,
        now: Instant,
        state: Option<S>,
        close_generation: u32,
    ) -> Option<FixtureResult<S>> {
        let pending = self.pending.as_mut()?;
        if pending.close_generation != close_generation {
            return self.finish(FixtureStatus::Cancelled, None);
        }
        let state = state.filter(|state| {
            state.metadata().provider == S::PROVIDER
                && pending
                    .provider_generation
                    .is_none_or(|generation| state.metadata().generation >= generation)
        });
        if let Some(state) = &state {
            if pending
                .provider_generation
                .is_some_and(|generation| state.metadata().generation > generation)
            {
                return self.finish(FixtureStatus::Restarted, Some(*state));
            }
            if pending.provider_generation.is_none() {
                pending.provider_generation = Some(state.metadata().generation);
            }
            if pending.qualifies(&state.metadata()) {
                return self.finish(FixtureStatus::Sampled, Some(*state));
            }
        }
        if now >= pending.request.expires_at {
            return self.finish(FixtureStatus::Expired, state);
        }
        None
    }

    fn finish(&mut self, status: FixtureStatus, state: Option<S>) -> Option<FixtureResult<S>> {
        self.pending
            .take()
            .map(|pending| pending.finish(status, state))
    }

    fn reject(request: FixtureRequest, status: FixtureStatus) -> FixtureResult<S> {
        FixtureResult {
            id: request.id,
            owner_generation: OwnerGeneration::INITIAL,
            admitted_at: None,
            expires_at: request.expires_at,
            baseline: None,
            status,
            state: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;
    use crate::ids::{OwnerId, ProviderId};
    use core::fmt;

    #[derive(Clone, Copy)]
    struct Sample;
    impl FixtureSample for Sample {
        type Fields = TestFields;
        const PROVIDER: ProviderId = ProviderId(2);
        const OWNER: OwnerId = OwnerId(1);
        const MAX_VALIDITY_MS: u32 = 300_000;
        fn fields() -> TestFields {
            TestFields::TEMPERATURE
        }
        fn metadata(self) -> SampleMetadata {
            panic!("no sample needed")
        }
        fn write_values(_: Option<Self>, _: &mut fmt::Formatter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    #[test]
    fn generation_exhaustion_is_terminal_without_wrapping() {
        let mut fixture = FixtureState::<Sample> {
            generation: OwnerGeneration(u32::MAX),
            ..Default::default()
        };
        for id in [1, 2] {
            let result = fixture
                .begin(
                    FixtureRequest {
                        id,
                        expires_at: Instant(200),
                    },
                    Instant(100),
                    None,
                    0,
                    |_, _| panic!("exhausted generation admitted"),
                )
                .expect("terminal result");
            assert_eq!(result.status, FixtureStatus::GenerationExhausted);
            assert_eq!(result.owner_generation, OwnerGeneration::INITIAL);
            assert!(fixture.poll(Instant(101), None, 0).is_none());
        }
    }
}
