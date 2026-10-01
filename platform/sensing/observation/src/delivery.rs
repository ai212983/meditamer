//! Deciding whether/what to deliver to one subscription this pass
//! (ADR-0018's three initial policies, delivery-interval rate limiting, and
//! the one-time entry/observe-now bypass).
//!
//! This module only *decides*; it never itself requests acquisition. The
//! provider already samples every requested field at the strictest active
//! `max_age` via [`crate::demand`]/[`crate::schedule`] regardless of any one
//! subscription's delivery decision here -- a `refresh` field in the
//! returned action names the fields this subscription is still waiting on,
//! it does not additionally trigger anything.

use crate::field::{field_indices, FieldMask};
use crate::ids::Revision;
use crate::policy::InitialPolicy;
use crate::schedule::FieldTimestamps;
use crate::subscription::{DeliveryTracking, ObservationSubscription};
use crate::time::Instant;

/// Per-pass inputs that are not read from the subscription or cache
/// directly.
#[derive(Clone, Copy, Debug)]
pub struct DeliveryInput {
    pub now: Instant,
    /// Whether the provider's health changed since this subscription's last
    /// delivery. A health change is always delivered promptly, bypassing
    /// the delivery-interval rate limit ("Publish health transitions
    /// promptly").
    pub health_changed: bool,
    /// The provider's current revision, recorded into
    /// [`DeliveryTracking::last_delivered_revision`] on delivery.
    pub revision: Revision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryAction<F: FieldMask> {
    /// Not due yet (rate-limited, or nothing usable and the policy does not
    /// wait on a refresh).
    Withhold,
    /// Deliver exactly these fields from cache.
    DeliverCache(F),
    /// Deliver `deliver` from cache now; `refresh` names the fields this
    /// subscription is still waiting on a fresher value for.
    DeliverCacheAndRequestRefresh { deliver: F, refresh: F },
    /// Nothing usable to deliver yet; `0` names the fields still due.
    RequestRefreshOnly(F),
}

/// Decides this pass's delivery for one subscription. Mutates `tracking` to
/// record a delivery when one occurs.
pub fn decide_delivery<F: FieldMask, const FIELDS: usize>(
    sub: &ObservationSubscription<F>,
    tracking: &mut DeliveryTracking,
    cache: &FieldTimestamps<FIELDS>,
    input: DeliveryInput,
) -> DeliveryAction<F> {
    let mut present_bits = 0u32;
    let mut fresh_bits = 0u32;
    for index in field_indices(sub.fields.bits()) {
        if let Some(last_success) = cache.get(index) {
            present_bits |= 1 << index;
            if last_success.saturating_add(sub.max_age) > input.now {
                fresh_bits |= 1 << index;
            }
        }
    }
    let present_fields = F::from_bits_truncate(present_bits);
    let due_fields = F::from_bits_truncate(sub.fields.bits() & !fresh_bits);
    let all_fresh = due_fields.is_empty();

    // A refresh this subscription itself asked for (entry, a stale
    // CacheThenRefresh re-check, or a caller-admitted observe-now request)
    // is satisfied the moment its fields catch up to fresh. That delivery
    // bypasses the ordinary rate limit once, regardless of how little time
    // has passed since whatever delivery requested it -- "Entry refresh and
    // explicit observe-now results bypass the interval once; later periodic
    // values follow it."
    let refresh_satisfied = tracking.refresh_pending && all_fresh;

    let rate_limited = tracking.bypass_used
        && !input.health_changed
        && !refresh_satisfied
        && tracking
            .last_delivered_at
            .is_some_and(|last| last.saturating_add(sub.min_delivery_interval) > input.now);

    let action = match sub.initial_policy {
        // "use available cache": deliver whatever is present, however
        // stale, and never wait on a refresh. `present_fields` already
        // equals `sub.fields` when every field is fresh, so this needs no
        // separate fresh-case branch.
        InitialPolicy::UseCache => {
            if present_fields.is_empty() || rate_limited {
                DeliveryAction::Withhold
            } else {
                DeliveryAction::DeliverCache(present_fields)
            }
        }
        // "refresh missing or stale fields": withhold until the due fields
        // catch up (never rate-limited -- requesting costs nothing), unless
        // everything is already fresh, in which case deliver normally.
        InitialPolicy::RefreshIfStale => {
            if all_fresh {
                if rate_limited {
                    DeliveryAction::Withhold
                } else {
                    DeliveryAction::DeliverCache(sub.fields)
                }
            } else {
                DeliveryAction::RequestRefreshOnly(due_fields)
            }
        }
        // "deliver cache and request a fresh observation even when cache is
        // fresh": always both, regardless of `all_fresh` -- unlike the other
        // two policies, this one must never collapse to a plain
        // `DeliverCache` just because nothing is currently due.
        InitialPolicy::CacheThenRefresh => {
            if present_fields.is_empty() {
                DeliveryAction::RequestRefreshOnly(due_fields)
            } else if rate_limited {
                DeliveryAction::Withhold
            } else {
                DeliveryAction::DeliverCacheAndRequestRefresh {
                    deliver: present_fields,
                    refresh: due_fields,
                }
            }
        }
    };

    match action {
        DeliveryAction::RequestRefreshOnly(refresh) if !refresh.is_empty() => {
            tracking.refresh_pending = true;
        }
        DeliveryAction::DeliverCacheAndRequestRefresh { refresh, .. } if !refresh.is_empty() => {
            tracking.refresh_pending = true;
        }
        _ if refresh_satisfied => {
            tracking.refresh_pending = false;
        }
        _ => {}
    }

    if matches!(
        action,
        DeliveryAction::DeliverCache(_) | DeliveryAction::DeliverCacheAndRequestRefresh { .. }
    ) {
        tracking.last_delivered_at = Some(input.now);
        tracking.last_delivered_revision = Some(input.revision);
        tracking.bypass_used = true;
    }

    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::test_fields::TestFields;
    use crate::ids::{OwnerGeneration, OwnerId, ProviderId, SubscriptionSlot};
    use crate::subscription::SubscriptionKey;
    use crate::time::Duration;

    fn sub(
        policy: InitialPolicy,
        max_age: Duration,
        min_interval: Duration,
    ) -> ObservationSubscription<TestFields> {
        ObservationSubscription {
            key: SubscriptionKey {
                provider: ProviderId(1),
                owner: OwnerId(1),
                owner_generation: OwnerGeneration(0),
                slot: SubscriptionSlot(0),
            },
            fields: TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            max_age,
            initial_policy: policy,
            min_delivery_interval: min_interval,
            expires_at: None,
        }
    }

    fn input(now: u64) -> DeliveryInput {
        DeliveryInput {
            now: Instant(now),
            health_changed: false,
            revision: Revision(1),
        }
    }

    #[test]
    fn missing_cache_withholds_under_use_cache_and_refresh_if_stale() {
        let cache = FieldTimestamps::<4>::new();
        for policy in [InitialPolicy::UseCache, InitialPolicy::RefreshIfStale] {
            let s = sub(policy, Duration::from_secs(60), Duration::from_secs(60));
            let mut tracking = DeliveryTracking::default();
            let action = decide_delivery(&s, &mut tracking, &cache, input(0));
            match policy {
                InitialPolicy::UseCache => assert_eq!(action, DeliveryAction::Withhold),
                InitialPolicy::RefreshIfStale => {
                    assert_eq!(action, DeliveryAction::RequestRefreshOnly(s.fields))
                }
                _ => unreachable!(),
            }
            assert!(tracking.last_delivered_at.is_none());
        }
    }

    #[test]
    fn fresh_cache_delivers_under_every_policy() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );

        for policy in [InitialPolicy::UseCache, InitialPolicy::RefreshIfStale] {
            let s = sub(policy, Duration::from_secs(60), Duration::from_secs(60));
            let mut tracking = DeliveryTracking::default();
            let action = decide_delivery(&s, &mut tracking, &cache, input(0));
            assert_eq!(action, DeliveryAction::DeliverCache(s.fields));
        }
    }

    /// Regression case for E-0008's "reproduced `CacheThenRefresh` returning
    /// `DeliverCache` for fresh-cache entry": this policy's whole point is
    /// "deliver cache and request a fresh observation even when cache is
    /// fresh" (ADR-0018), so it must never collapse to a plain `DeliverCache`
    /// just because nothing is due -- unlike `UseCache`/`RefreshIfStale`
    /// above, which correctly do.
    #[test]
    fn fresh_cache_entry_under_cache_then_refresh_still_requests_a_refresh() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::CacheThenRefresh,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        let action = decide_delivery(&s, &mut tracking, &cache, input(0));
        assert_eq!(
            action,
            DeliveryAction::DeliverCacheAndRequestRefresh {
                deliver: s.fields,
                refresh: TestFields::EMPTY, // already fresh: nothing currently due
            }
        );
    }

    #[test]
    fn stale_cache_use_cache_delivers_stale_value_without_refresh_wait() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::UseCache,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        let action = decide_delivery(&s, &mut tracking, &cache, input(120_000));
        assert_eq!(action, DeliveryAction::DeliverCache(s.fields));
    }

    #[test]
    fn stale_cache_cache_then_refresh_delivers_and_notes_refresh() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::CacheThenRefresh,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        let action = decide_delivery(&s, &mut tracking, &cache, input(120_000));
        assert_eq!(
            action,
            DeliveryAction::DeliverCacheAndRequestRefresh {
                deliver: s.fields,
                refresh: s.fields,
            }
        );
    }

    /// Regression case for E-0008's "withholding a refreshed result 20 ms
    /// after stale-cache entry": the entry delivery above already consumed
    /// `bypass_used`, so without `refresh_pending` tracking that a refresh
    /// is still outstanding, the fresh result satisfying it would be
    /// rate-limited by `min_delivery_interval` like any other periodic
    /// value, even though almost no time has passed.
    #[test]
    fn the_refresh_a_stale_entry_requested_bypasses_the_interval_once() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::CacheThenRefresh,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        // Entry with stale cache (120s old, max_age 60s): delivers the
        // stale value and leaves a refresh pending.
        let entry = decide_delivery(&s, &mut tracking, &cache, input(120_000));
        assert!(matches!(
            entry,
            DeliveryAction::DeliverCacheAndRequestRefresh { .. }
        ));
        assert!(tracking.refresh_pending);

        // 20ms later, the requested acquisition completes and the cache
        // catches up to fresh -- well inside min_delivery_interval (60s),
        // but this is exactly the pending refresh, so it must not withhold.
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(120_020),
        );
        let followup = decide_delivery(&s, &mut tracking, &cache, input(120_020));
        assert_eq!(
            followup,
            DeliveryAction::DeliverCacheAndRequestRefresh {
                deliver: s.fields,
                refresh: TestFields::EMPTY,
            }
        );
        assert!(!tracking.refresh_pending); // consumed

        // A later, ordinary periodic re-check within the interval is
        // rate-limited as normal -- the bypass was one-time only.
        let periodic = decide_delivery(&s, &mut tracking, &cache, input(120_100));
        assert_eq!(periodic, DeliveryAction::Withhold);
    }

    #[test]
    fn partial_field_success_delivers_only_present_fields() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(TestFields::TEMPERATURE, Instant(0)); // humidity never sampled
        let s = sub(
            InitialPolicy::CacheThenRefresh,
            Duration::from_secs(60),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        let action = decide_delivery(&s, &mut tracking, &cache, input(0));
        assert_eq!(
            action,
            DeliveryAction::DeliverCacheAndRequestRefresh {
                deliver: TestFields::TEMPERATURE,
                refresh: TestFields::HUMIDITY,
            }
        );
    }

    #[test]
    fn entry_bypasses_interval_then_later_delivery_is_rate_limited() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::UseCache,
            Duration::from_secs(600),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        // Entry: first delivery, bypasses the (irrelevant here, nothing
        // elapsed) interval.
        let first = decide_delivery(&s, &mut tracking, &cache, input(0));
        assert_eq!(first, DeliveryAction::DeliverCache(s.fields));

        // Immediately after: within min_delivery_interval, now rate-limited.
        let second = decide_delivery(&s, &mut tracking, &cache, input(1_000));
        assert_eq!(second, DeliveryAction::Withhold);

        // After the interval elapses: delivers again.
        let third = decide_delivery(&s, &mut tracking, &cache, input(61_000));
        assert_eq!(third, DeliveryAction::DeliverCache(s.fields));
    }

    #[test]
    fn health_only_change_bypasses_the_rate_limit() {
        let mut cache = FieldTimestamps::<4>::new();
        cache.stamp(
            TestFields::TEMPERATURE.union(TestFields::HUMIDITY),
            Instant(0),
        );
        let s = sub(
            InitialPolicy::UseCache,
            Duration::from_secs(600),
            Duration::from_secs(60),
        );
        let mut tracking = DeliveryTracking::default();

        decide_delivery(&s, &mut tracking, &cache, input(0));
        let mut changed = input(1_000);
        changed.health_changed = true;
        let action = decide_delivery(&s, &mut tracking, &cache, changed);
        assert_eq!(action, DeliveryAction::DeliverCache(s.fields));
    }
}
