//! Initial delivery policy and provider health (ADR-0018).

/// What a subscription wants the first time it becomes due for delivery
/// (entry into a screen, or a newly installed background subscription).
/// Later, periodic deliveries always behave like a fresh read against
/// `max_age` regardless of which initial policy was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialPolicy {
    /// Deliver whatever is cached, even if stale or missing; never requests
    /// a refresh on this consumer's behalf.
    UseCache,
    /// Deliver cache only if it satisfies `max_age`; otherwise withhold and
    /// request a refresh, delivering once the refresh lands.
    RefreshIfStale,
    /// Deliver cache immediately regardless of freshness, and also request a
    /// refresh so the next delivery reflects a current reading.
    CacheThenRefresh,
}

/// Health of a provider's latest state, carried in [`crate::state::ObservationState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// No completed attempt yet (fresh provider, or since its last restart).
    Unknown,
    /// Latest attempt succeeded.
    Ok,
    /// Recent attempts have failed, but a usable (possibly stale) cache
    /// remains from an earlier success.
    Degraded,
    /// No usable cache; every attempt so far (or since the last success) has
    /// failed.
    Failed,
}
