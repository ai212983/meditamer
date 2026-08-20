use console::println;
use embassy_time::Instant;

use super::progress::UPLOAD_CHUNK_PIPELINE_ENABLED;
use crate::firmware::observability;

pub(crate) struct UploadBodyStats {
    pub(crate) sent_bytes: usize,
    pub(crate) chunk_count: u32,
    pub(crate) max_chunk_bytes: usize,
    pub(crate) body_read_ms: u32,
    pub(crate) payload_copy_ms: u32,
    pub(crate) sd_queue_ms: u32,
    pub(crate) sd_task_wait_ms: u32,
    pub(crate) sd_task_queue_wait_ms: u32,
    pub(crate) sd_task_handler_ms: u32,
    pub(crate) sd_task_residual_ms: u32,
    pub(crate) sd_task_post_handler_ms: u32,
    pub(crate) sd_task_publish_to_receive_ms: u32,
    pub(crate) sd_task_residual_other_ms: u32,
    pub(crate) sd_wait_ms: u32,
    pub(crate) chunk_p50_ms: u32,
    pub(crate) chunk_p95_ms: u32,
    pub(crate) chunk_max_ms: u32,
    pub(crate) chunk_samples: u32,
    pub(crate) chunk_samples_dropped: u32,
    pub(crate) ingress_flush_wait_ms: u32,
    pub(crate) ingress_read_calls: u32,
    pub(crate) ingress_read_pre_queue_bytes_total: u32,
    pub(crate) ingress_read_pre_queue_max: u32,
    pub(crate) ingress_read_pre_queue_empty_calls: u32,
    pub(crate) ingress_read_short_calls: u32,
    pub(crate) ingress_read_wait_empty_q_ms: u32,
    pub(crate) ingress_read_wait_nonempty_q_ms: u32,
    pub(crate) ingress_read_wait_over_10ms: u32,
    pub(crate) ingress_read_wait_over_50ms: u32,
    pub(crate) ingress_read_wait_over_100ms: u32,
    pub(crate) ingress_read_wait_empty_q_over_10ms: u32,
    pub(crate) ingress_read_wait_empty_q_over_50ms: u32,
    pub(crate) ingress_read_wait_empty_q_over_100ms: u32,
    pub(crate) ingress_read_wait_empty_q_max_ms: u32,
    pub(crate) ingress_read_empty_streak_ms_max: u32,
    pub(crate) ingress_adapt_enabled: u32,
    pub(crate) ingress_adapt_switches: u32,
    pub(crate) ingress_adapt_level_max: u32,
    pub(crate) ingress_read_empty_streak_max: u32,
}

pub(crate) fn log_upload_stats(
    phase: &str,
    stats: &UploadBodyStats,
    total_sd_wait_ms: u32,
    request_started_at: Instant,
    commit_ms: u32,
) {
    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        let avg_chunk = if stats.chunk_count == 0 {
            0
        } else {
            stats.sent_bytes / stats.chunk_count as usize
        };
        let snapshot = observability::snapshot();
        println!(
            "upload_http: {} stats pipeline={} bytes={} chunks={} avg_chunk={} max_chunk={} read_wait_ms={} copy_ms={} sd_queue_ms={} sd_task_ms={} sd_task_queue_wait_ms={} sd_task_handler_ms={} sd_task_residual_ms={} sd_task_post_handler_ms={} sd_task_publish_to_receive_ms={} sd_task_residual_other_ms={} sd_ms={} chunk_p50_ms={} chunk_p95_ms={} chunk_max_ms={} chunk_samples={} chunk_samples_dropped={} ingress_flush_wait_ms={} ingress_read_calls={} ingress_pre_read_q_total={} ingress_pre_read_q_max={} ingress_pre_read_q_empty_calls={} ingress_read_short_calls={} ingress_read_wait_empty_q_ms={} ingress_read_wait_nonempty_q_ms={} ingress_read_wait_over_10ms={} ingress_read_wait_over_50ms={} ingress_read_wait_over_100ms={} ingress_read_wait_empty_q_over_10ms={} ingress_read_wait_empty_q_over_50ms={} ingress_read_wait_empty_q_over_100ms={} ingress_read_wait_empty_q_max_ms={} ingress_read_empty_streak_ms_max={} ingress_adapt_enabled={} ingress_adapt_switches={} ingress_adapt_level_max={} ingress_read_empty_streak_max={} wifi_rssi_last_dbm={} wifi_rssi_min_dbm={} wifi_rssi_max_dbm={} wifi_rssi_samples={} wifi_rssi_low_samples={} commit_ms={} req_ms={}",
            phase,
            if UPLOAD_CHUNK_PIPELINE_ENABLED { "on" } else { "off" },
            stats.sent_bytes,
            stats.chunk_count,
            avg_chunk,
            stats.max_chunk_bytes,
            stats.body_read_ms,
            stats.payload_copy_ms,
            stats.sd_queue_ms,
            stats.sd_task_wait_ms,
            stats.sd_task_queue_wait_ms,
            stats.sd_task_handler_ms,
            stats.sd_task_residual_ms,
            stats.sd_task_post_handler_ms,
            stats.sd_task_publish_to_receive_ms,
            stats.sd_task_residual_other_ms,
            total_sd_wait_ms,
            stats.chunk_p50_ms,
            stats.chunk_p95_ms,
            stats.chunk_max_ms,
            stats.chunk_samples,
            stats.chunk_samples_dropped,
            stats.ingress_flush_wait_ms,
            stats.ingress_read_calls,
            stats.ingress_read_pre_queue_bytes_total,
            stats.ingress_read_pre_queue_max,
            stats.ingress_read_pre_queue_empty_calls,
            stats.ingress_read_short_calls,
            stats.ingress_read_wait_empty_q_ms,
            stats.ingress_read_wait_nonempty_q_ms,
            stats.ingress_read_wait_over_10ms,
            stats.ingress_read_wait_over_50ms,
            stats.ingress_read_wait_over_100ms,
            stats.ingress_read_wait_empty_q_over_10ms,
            stats.ingress_read_wait_empty_q_over_50ms,
            stats.ingress_read_wait_empty_q_over_100ms,
            stats.ingress_read_wait_empty_q_max_ms,
            stats.ingress_read_empty_streak_ms_max,
            stats.ingress_adapt_enabled,
            stats.ingress_adapt_switches,
            stats.ingress_adapt_level_max,
            stats.ingress_read_empty_streak_max,
            snapshot.wifi_link_rssi_last_dbm,
            snapshot.wifi_link_rssi_min_dbm,
            snapshot.wifi_link_rssi_max_dbm,
            snapshot.wifi_link_rssi_samples,
            snapshot.wifi_link_rssi_low_samples,
            commit_ms,
            elapsed_ms_u32(request_started_at),
        );
    }
}

pub(crate) fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

pub(crate) fn usize_to_u32_saturating(value: usize) -> u32 {
    if value > u32::MAX as usize {
        u32::MAX
    } else {
        value as u32
    }
}
