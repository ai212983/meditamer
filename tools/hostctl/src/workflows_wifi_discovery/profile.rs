use serde::Deserialize;

use crate::workflows_wifi_common::NetPolicy;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default)]
pub(super) struct DiscoveryProfile {
    pub rounds: u32,
    pub round_timeout_ms: u32,
    pub poll_interval_ms: u32,
    pub status_poll_ms: u32,
    pub recover_before_round: bool,
    pub recover_after_round: bool,
    pub recover_settle_ms: u32,
    pub disable_listener_during_probe_rounds: bool,
    pub max_zero_discovery_rounds: u32,
    pub min_ready_rounds: u32,
    pub min_ssid_seen_rounds: u32,
}

impl Default for DiscoveryProfile {
    fn default() -> Self {
        Self {
            rounds: 8,
            round_timeout_ms: 60_000,
            poll_interval_ms: 250,
            status_poll_ms: 1_000,
            recover_before_round: true,
            recover_after_round: false,
            recover_settle_ms: 6_000,
            disable_listener_during_probe_rounds: true,
            max_zero_discovery_rounds: 0,
            min_ready_rounds: 1,
            min_ssid_seen_rounds: 1,
        }
    }
}

fn active_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    (policy.scan_active_max_ms.max(policy.scan_active_min_ms) as u64)
        .saturating_mul(10)
        .clamp(8_000, 25_000)
}

fn directed_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(3_000, 8_000)
}

fn passive_scan_timeout_ms(policy: &NetPolicy) -> u64 {
    (policy.scan_passive_ms as u64)
        .saturating_mul(16)
        .saturating_add(3_000)
        .clamp(15_000, 90_000)
}

fn zero_discovery_probe_timeout_ms(policy: &NetPolicy) -> u64 {
    active_scan_timeout_ms(policy).clamp(2_500, 6_000)
}

fn zero_discovery_probe_budget_ms(policy: &NetPolicy) -> u64 {
    const ZERO_DISCOVERY_PROBE_CHANNELS: u64 = 13;
    zero_discovery_probe_timeout_ms(policy).saturating_mul(ZERO_DISCOVERY_PROBE_CHANNELS)
}

pub(super) fn recommended_round_timeout_ms(policy: &NetPolicy, profile: &DiscoveryProfile) -> u64 {
    let scan_budget_ms = active_scan_timeout_ms(policy)
        .saturating_add(directed_scan_timeout_ms(policy))
        .saturating_add(passive_scan_timeout_ms(policy))
        .saturating_add(zero_discovery_probe_budget_ms(policy))
        .saturating_add(6_000);
    let watchdog_timeout_ms = (policy.connect_timeout_ms as u64)
        .saturating_add(scan_budget_ms)
        .max((policy.connect_timeout_ms as u64).saturating_mul(2));
    let recover_budget_ms =
        policy.driver_restart_backoff_ms as u64 + profile.recover_settle_ms as u64;
    let recommended = watchdog_timeout_ms
        .saturating_add(recover_budget_ms)
        .saturating_add(5_000);
    recommended.max(profile.round_timeout_ms as u64)
}
