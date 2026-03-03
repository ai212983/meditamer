use sdcard::probe::SdWriteMetrics;

pub(super) fn write_metrics_delta(start: SdWriteMetrics, end: SdWriteMetrics) -> SdWriteMetrics {
    SdWriteMetrics {
        cmd24_sectors: end.cmd24_sectors.saturating_sub(start.cmd24_sectors),
        cmd25_attempt_bursts: end
            .cmd25_attempt_bursts
            .saturating_sub(start.cmd25_attempt_bursts),
        cmd25_attempt_sectors: end
            .cmd25_attempt_sectors
            .saturating_sub(start.cmd25_attempt_sectors),
        cmd25_success_bursts: end
            .cmd25_success_bursts
            .saturating_sub(start.cmd25_success_bursts),
        cmd25_success_sectors: end
            .cmd25_success_sectors
            .saturating_sub(start.cmd25_success_sectors),
        cmd25_fallback_bursts: end
            .cmd25_fallback_bursts
            .saturating_sub(start.cmd25_fallback_bursts),
        cmd25_success_burst_ms_total: end
            .cmd25_success_burst_ms_total
            .saturating_sub(start.cmd25_success_burst_ms_total),
        cmd25_ready_wait_count: end
            .cmd25_ready_wait_count
            .saturating_sub(start.cmd25_ready_wait_count),
        cmd25_ready_wait_ms_total: end
            .cmd25_ready_wait_ms_total
            .saturating_sub(start.cmd25_ready_wait_ms_total),
        cmd25_ready_wait_polls_total: end
            .cmd25_ready_wait_polls_total
            .saturating_sub(start.cmd25_ready_wait_polls_total),
        cmd25_ready_wait_over_1ms: end
            .cmd25_ready_wait_over_1ms
            .saturating_sub(start.cmd25_ready_wait_over_1ms),
        cmd25_ready_wait_over_4ms: end
            .cmd25_ready_wait_over_4ms
            .saturating_sub(start.cmd25_ready_wait_over_4ms),
        cmd25_ready_wait_over_8ms: end
            .cmd25_ready_wait_over_8ms
            .saturating_sub(start.cmd25_ready_wait_over_8ms),
    }
}
