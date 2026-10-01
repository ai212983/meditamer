//! Provider, owner, and slot identity.
//!
//! Kept as one small module because the lifecycle rules that connect them
//! are the point: provider IDs are stable across a restart while the
//! provider's *generation* advances on restart; owner identity plus
//! generation route delivery to a live surface and let a stale one be
//! ignored after rollback/recovery (ADR-0018).

/// Identifies one physical or logical provider (e.g. "Medinote SHTC3",
/// "Meditamer BQ27441"). Stable across target restarts -- a parallel sensor
/// gets its own `ProviderId` rather than reusing one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(pub u16);

/// Advances when a provider task restarts. Consumers reset their delivered-
/// revision baseline when they observe a new generation for a provider,
/// since revisions are only ordered within one generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderGeneration(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderGenerationExhausted;

impl ProviderGeneration {
    pub const INITIAL: ProviderGeneration = ProviderGeneration(0);

    /// Reported as exhausted rather than wrapping, matching
    /// [`Revision::checked_advance`]'s reasoning: a wrapped generation could
    /// collide with an already-issued subscription's stale
    /// `owner_generation` comparison, silently treating a stale owner as
    /// live again. A caller decides what "provider identity needs
    /// reassignment" means for that target.
    pub fn checked_advance(self) -> Result<Self, ProviderGenerationExhausted> {
        self.0
            .checked_add(1)
            .map(ProviderGeneration)
            .ok_or(ProviderGenerationExhausted)
    }
}

/// Advances on every completed acquisition attempt for a provider,
/// including failures, within one [`ProviderGeneration`]. Reported as
/// exhausted rather than wrapping so a caller can decide what "provider
/// needs a restart" means for that target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevisionExhausted;

impl Revision {
    pub const INITIAL: Revision = Revision(0);

    pub fn checked_advance(self) -> Result<Revision, RevisionExhausted> {
        self.0.checked_add(1).map(Revision).ok_or(RevisionExhausted)
    }
}

/// A bounded identity for a subscription owner: a committed surface
/// instance, or a persistent background consumer (e.g. the IMU trace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerId(pub u16);

/// Advances when the same owner identity is recommitted (e.g. a surface
/// torn down and rebuilt after rollback). A subscription and any in-flight
/// observe-now result carry the generation they were made under, so a late
/// result for a stale generation can be discarded rather than misapplied to
/// the new instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerGeneration(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerGenerationExhausted;

impl OwnerGeneration {
    pub const INITIAL: Self = Self(0);

    /// Renew a consumer without ever reusing an old delivery identity.
    /// The authority must withdraw the owner if renewal is exhausted.
    pub fn checked_advance(self) -> Result<Self, OwnerGenerationExhausted> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(OwnerGenerationExhausted)
    }
}

/// Distinguishes multiple subscriptions the same owner holds against the
/// same provider (e.g. a sticky status projection and a foreground detail
/// view both reading temperature at different cadences).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriptionSlot(pub u8);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_generation_reports_exhaustion_instead_of_wrapping() {
        assert_eq!(
            ProviderGeneration::INITIAL.checked_advance(),
            Ok(ProviderGeneration(1))
        );
        assert_eq!(
            ProviderGeneration(u32::MAX).checked_advance(),
            Err(ProviderGenerationExhausted)
        );
    }

    #[test]
    fn revision_reports_exhaustion_instead_of_wrapping() {
        assert_eq!(Revision::INITIAL.checked_advance(), Ok(Revision(1)));
        assert_eq!(Revision(u32::MAX).checked_advance(), Err(RevisionExhausted));
    }

    #[test]
    fn owner_generation_reports_exhaustion_instead_of_wrapping() {
        assert_eq!(
            OwnerGeneration::INITIAL.checked_advance(),
            Ok(OwnerGeneration(1))
        );
        assert_eq!(
            OwnerGeneration(u32::MAX).checked_advance(),
            Err(OwnerGenerationExhausted)
        );
    }
}
