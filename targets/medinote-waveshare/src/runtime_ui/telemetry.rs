//! Boot-lifetime UI resource and input-to-flush telemetry.

use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant};
use render::lvgl_adapter::UiAccessToken;
use waveshare_rlcd42::panel::FlushCompletion;

const LOG_INTERVAL_SECS: u64 = 30;

#[derive(Clone, Copy, Default)]
struct InternalHeapMetrics {
    enabled: bool,
    current: usize,
    max: usize,
    free: usize,
}

#[cfg(any(feature = "shared-ble-runtime", feature = "wifi-storage"))]
fn internal_heap_metrics() -> InternalHeapMetrics {
    let heap = esp_alloc::HEAP.stats();
    InternalHeapMetrics {
        enabled: true,
        current: heap.current_usage,
        max: heap.max_usage,
        free: esp_alloc::HEAP.free(),
    }
}

#[cfg(not(any(feature = "shared-ble-runtime", feature = "wifi-storage")))]
fn internal_heap_metrics() -> InternalHeapMetrics {
    InternalHeapMetrics::default()
}

#[derive(Clone, Copy, Default)]
struct BleQueueMetrics {
    enabled: bool,
    pending: usize,
    high_water: usize,
    overflow_total: u32,
}

#[cfg(feature = "cheertok-controls")]
fn ble_queue_metrics() -> BleQueueMetrics {
    let queue = crate::cheertok::input_queue_metrics();
    BleQueueMetrics {
        enabled: true,
        pending: queue.pending,
        high_water: queue.high_water,
        overflow_total: queue.overflow_total,
    }
}

#[cfg(not(feature = "cheertok-controls"))]
fn ble_queue_metrics() -> BleQueueMetrics {
    BleQueueMetrics::default()
}

pub(super) struct RuntimeTelemetry {
    next_log: EmbassyInstant,
    app_input_flush_count: u32,
    app_input_flush_max_us: u64,
    app_input_flush_bytes_max: usize,
}

impl RuntimeTelemetry {
    pub(super) fn new() -> Self {
        Self {
            next_log: EmbassyInstant::now(),
            app_input_flush_count: 0,
            app_input_flush_max_us: 0,
            app_input_flush_bytes_max: 0,
        }
    }

    pub(super) fn record_input_flush(
        &mut self,
        input_at_us: u64,
        before: FlushCompletion,
        after: FlushCompletion,
    ) {
        if before.sequence == after.sequence {
            return;
        }
        let latency_us = super::monotonic_micros().saturating_sub(input_at_us);
        self.app_input_flush_count = self.app_input_flush_count.saturating_add(1);
        self.app_input_flush_max_us = self.app_input_flush_max_us.max(latency_us);
        self.app_input_flush_bytes_max = self.app_input_flush_bytes_max.max(after.bytes);
    }

    pub(super) fn poll(&mut self, ui_token: &UiAccessToken) {
        let now = EmbassyInstant::now();
        if now < self.next_log {
            return;
        }
        while self.next_log <= now {
            self.next_log += EmbassyDuration::from_secs(LOG_INTERVAL_SECS);
        }

        let Ok(lvgl) = ui_token.memory_snapshot() else {
            console::println!("MEDINOTE_RUNTIME_METRICS lvgl_snapshot=unavailable");
            return;
        };
        let heap = internal_heap_metrics();
        let queue = ble_queue_metrics();
        let uptime_ms = super::monotonic_micros() / 1_000;
        console::println!(
            "MEDINOTE_RUNTIME_METRICS uptime_ms={} lvgl_total={} lvgl_used={} lvgl_free={} lvgl_largest_free={} lvgl_free_blocks={} lvgl_used_blocks={} lvgl_max_used={} lvgl_used_pct={} lvgl_fragmentation_pct={} lvgl_integrity_ok={} internal_heap_enabled={} internal_heap_current={} internal_heap_max={} internal_heap_free={} ble_queue_enabled={} ble_queue_pending={} ble_queue_high_water={} ble_queue_overflow_total={} app_input_to_panel_flush_complete_count={} app_input_to_panel_flush_complete_max_us={} app_input_flush_bytes_max={}",
            uptime_ms,
            lvgl.total_size,
            lvgl.total_size.saturating_sub(lvgl.free_size),
            lvgl.free_size,
            lvgl.largest_free_size,
            lvgl.free_count,
            lvgl.used_count,
            lvgl.max_used,
            lvgl.used_percent,
            lvgl.fragmentation_percent,
            lvgl.integrity_ok,
            heap.enabled,
            heap.current,
            heap.max,
            heap.free,
            queue.enabled,
            queue.pending,
            queue.high_water,
            queue.overflow_total,
            self.app_input_flush_count,
            self.app_input_flush_max_us,
            self.app_input_flush_bytes_max,
        );
    }
}
