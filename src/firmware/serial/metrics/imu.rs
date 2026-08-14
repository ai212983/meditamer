use core::fmt::Write;

use crate::firmware::types::SerialUart;

use super::write_line;

pub(super) async fn write_metrics_imu_line(uart: &mut SerialUart) {
    let metrics = crate::firmware::imu::metrics::snapshot();
    let config = &crate::firmware::event_engine::config::active_config().imu_sampling;
    let mut line = heapless::String::<384>::new();
    let _ = write!(
        &mut line,
        "METRICS IMU_SCHED odr_hz={} idle_hz={} active_hz={} idle_n={} active_n={} gap_max_ms={} promote={} demote={} missed={} touch_skip={} upload_skip={} discontinuity={} init_fail={} sample_fail={} recovery={} coalesced={}\r\n",
        config.sensor_odr_hz,
        config.idle_hz,
        config.active_hz,
        metrics.idle_samples,
        metrics.active_samples,
        metrics.sample_gap_max_ms,
        metrics.promotions,
        metrics.demotions,
        metrics.missed_deadlines,
        metrics.touch_suppressed,
        metrics.upload_suppressed,
        metrics.discontinuities,
        metrics.init_failures,
        metrics.sample_failures,
        metrics.recoveries,
        metrics.action_coalesced,
    );
    write_line(uart, &line).await;
}
