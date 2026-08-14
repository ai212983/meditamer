//! Coherent off-state resource measurement after a network epoch has ended.

use embassy_time::{Duration, Instant, Timer};

use super::{resource_snapshot, ResourceSnapshot, QUIESCENCE_POLL};

const OFF_RESOURCE_SETTLEMENT_TIMEOUT: Duration = Duration::from_secs(2);
const OFF_RESOURCE_SETTLEMENT_SAMPLES: u8 = 2;
const OFF_RESOURCE_POST_PROBE_CONFIRMATIONS: u8 = 2;
const OFF_RESOURCE_RTOS_BARRIER_US: u32 = 1_000;

fn rtos_settlement_barrier() {
    // Embassy time alone does not force ESP-RTOS to drain tasks marked for
    // deferred self-deletion. Sleeping the current executor task guarantees a
    // preemptive scheduler pass before allocator evidence is sampled.
    esp_radio_rtos_driver::usleep(OFF_RESOURCE_RTOS_BARRIER_US);
}

fn ownership_quiescent(resources: &ResourceSnapshot) -> bool {
    resources.http_connections == 0
        && resources.sd_roundtrips == 0
        && resources.sd_sessions == 0
        && resources.radio_callbacks == 0
        && resources.radio_queues == 0
        && !resources.radio_source_active
        && !resources.callback_admission_open
        && resources.late_callbacks == 0
        && resources.queue_late_use == 0
        && resources.queue_unknown_use == 0
        && resources.queue_reclaim_failures == 0
        && resources.queue_corruption == 0
        && resources.queue_contention == 0
}

fn coherent_probe(resources: &ResourceSnapshot) -> bool {
    resources.stable
        && resources.internal_free_bytes == resources.probe_free_before_bytes
        && resources.probe_free_before_bytes == resources.probe_free_after_bytes
}

fn same_probe_signature(left: &ResourceSnapshot, right: &ResourceSnapshot) -> bool {
    left.internal_free_bytes == right.internal_free_bytes
        && left.largest_block_above_reserve_bytes == right.largest_block_above_reserve_bytes
        && left.probe_free_before_bytes == right.probe_free_before_bytes
        && left.probe_free_after_bytes == right.probe_free_after_bytes
        && left.probe_reserve_bytes == right.probe_reserve_bytes
}

fn settlement_timed_out(started: Instant) -> bool {
    Instant::now().duration_since(started) >= OFF_RESOURCE_SETTLEMENT_TIMEOUT
}

fn reject_unsettled(mut resources: ResourceSnapshot) -> ResourceSnapshot {
    resources.stable = false;
    resources
}

pub(super) async fn settled_off_resource_snapshot() -> ResourceSnapshot {
    let started = Instant::now();
    let mut last_free = None;
    let mut consecutive = 0u8;

    loop {
        if settlement_timed_out(started) {
            return reject_unsettled(resource_snapshot(false));
        }
        rtos_settlement_barrier();
        let current = resource_snapshot(false);
        if !ownership_quiescent(&current) {
            return current;
        }
        if last_free == Some(current.internal_free_bytes) {
            consecutive = consecutive.saturating_add(1);
        } else {
            last_free = Some(current.internal_free_bytes);
            consecutive = 1;
        }
        if consecutive >= OFF_RESOURCE_SETTLEMENT_SAMPLES {
            rtos_settlement_barrier();
            let mut probed = resource_snapshot(true);
            if !ownership_quiescent(&probed) {
                return probed;
            }
            let mut confirmations = 0u8;
            while coherent_probe(&probed) && confirmations < OFF_RESOURCE_POST_PROBE_CONFIRMATIONS {
                if settlement_timed_out(started) {
                    return reject_unsettled(probed);
                }
                Timer::after(QUIESCENCE_POLL).await;
                if settlement_timed_out(started) {
                    return reject_unsettled(probed);
                }
                rtos_settlement_barrier();
                if settlement_timed_out(started) {
                    return reject_unsettled(probed);
                }
                let confirmed = resource_snapshot(true);
                if !ownership_quiescent(&confirmed) {
                    return confirmed;
                }
                if !coherent_probe(&confirmed) || !same_probe_signature(&probed, &confirmed) {
                    probed = confirmed;
                    break;
                }
                probed = confirmed;
                confirmations = confirmations.saturating_add(1);
            }
            if confirmations >= OFF_RESOURCE_POST_PROBE_CONFIRMATIONS {
                if settlement_timed_out(started) {
                    return reject_unsettled(probed);
                }
                return probed;
            }
            last_free = Some(probed.probe_free_after_bytes);
            consecutive = 0;
            if settlement_timed_out(started) {
                return reject_unsettled(probed);
            }
        }

        if settlement_timed_out(started) {
            return reject_unsettled(current);
        }
        Timer::after(QUIESCENCE_POLL).await;
    }
}
