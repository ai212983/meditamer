use super::super::fairness::{IngressFairnessAdaptive, IngressFairnessAdaptiveSnapshot};
use super::super::super::super::sd_bridge::SdUploadChunkFinish;
use super::latency::{chunk_latency_quantiles, record_chunk_latency_sample, ChunkLatencySamples};
use super::stats::UploadBodyStats;

pub(super) const UPLOAD_CHUNK_PIPELINE_ENABLED: bool = cfg!(feature = "asset-upload-http-pipeline");
const INGRESS_READ_WAIT_OVER_10MS: u32 = 10;
const INGRESS_READ_WAIT_OVER_50MS: u32 = 50;
const INGRESS_READ_WAIT_OVER_100MS: u32 = 100;

pub(super) struct UploadBodyProgress {
    body_read_ms: u32,
    payload_copy_ms: u32,
    sd_queue_ms: u32,
    sd_task_wait_ms: u32,
    sd_task_queue_wait_ms: u32,
    sd_task_handler_ms: u32,
    sd_task_residual_ms: u32,
    sd_task_post_handler_ms: u32,
    sd_task_publish_to_receive_ms: u32,
    sd_task_residual_other_ms: u32,
    sd_wait_ms: u32,
    sent_bytes: usize,
    chunk_count: u32,
    max_chunk_bytes: usize,
    ingress_flush_wait_ms: u32,
    ingress_read_calls: u32,
    ingress_read_pre_queue_bytes_total: u32,
    ingress_read_pre_queue_max: u32,
    ingress_read_pre_queue_empty_calls: u32,
    ingress_read_short_calls: u32,
    ingress_read_wait_empty_q_ms: u32,
    ingress_read_wait_nonempty_q_ms: u32,
    ingress_read_wait_over_10ms: u32,
    ingress_read_wait_over_50ms: u32,
    ingress_read_wait_over_100ms: u32,
    ingress_read_wait_empty_q_over_10ms: u32,
    ingress_read_wait_empty_q_over_50ms: u32,
    ingress_read_wait_empty_q_over_100ms: u32,
    ingress_read_wait_empty_q_max_ms: u32,
    ingress_read_empty_streak_ms: u32,
    ingress_read_empty_streak_ms_max: u32,
    ingress_read_bytes_since_yield: usize,
    ingress_read_ops_since_yield: u32,
    ingress_read_ops_since_try_drain: u32,
    chunk_samples: ChunkLatencySamples,
}

impl UploadBodyProgress {
    pub(super) fn new() -> Self {
        Self {
            body_read_ms: 0,
            payload_copy_ms: 0,
            sd_queue_ms: 0,
            sd_task_wait_ms: 0,
            sd_task_queue_wait_ms: 0,
            sd_task_handler_ms: 0,
            sd_task_residual_ms: 0,
            sd_task_post_handler_ms: 0,
            sd_task_publish_to_receive_ms: 0,
            sd_task_residual_other_ms: 0,
            sd_wait_ms: 0,
            sent_bytes: 0,
            chunk_count: 0,
            max_chunk_bytes: 0,
            ingress_flush_wait_ms: 0,
            ingress_read_calls: 0,
            ingress_read_pre_queue_bytes_total: 0,
            ingress_read_pre_queue_max: 0,
            ingress_read_pre_queue_empty_calls: 0,
            ingress_read_short_calls: 0,
            ingress_read_wait_empty_q_ms: 0,
            ingress_read_wait_nonempty_q_ms: 0,
            ingress_read_wait_over_10ms: 0,
            ingress_read_wait_over_50ms: 0,
            ingress_read_wait_over_100ms: 0,
            ingress_read_wait_empty_q_over_10ms: 0,
            ingress_read_wait_empty_q_over_50ms: 0,
            ingress_read_wait_empty_q_over_100ms: 0,
            ingress_read_wait_empty_q_max_ms: 0,
            ingress_read_empty_streak_ms: 0,
            ingress_read_empty_streak_ms_max: 0,
            ingress_read_bytes_since_yield: 0,
            ingress_read_ops_since_yield: 0,
            ingress_read_ops_since_try_drain: 0,
            chunk_samples: ChunkLatencySamples::new(),
        }
    }

    pub(super) fn record_payload_copy_ms(&mut self, copy_ms: u32) {
        self.payload_copy_ms = self.payload_copy_ms.saturating_add(copy_ms);
    }

    pub(super) fn should_try_drain(&self, pre_read_queue: u32, try_drain_interval_reads: u32) -> bool {
        UPLOAD_CHUNK_PIPELINE_ENABLED
            && (pre_read_queue == 0 || self.ingress_read_ops_since_try_drain >= try_drain_interval_reads)
    }

    pub(super) fn reset_try_drain_counter(&mut self) {
        self.ingress_read_ops_since_try_drain = 0;
    }

    pub(super) fn note_pre_read(&mut self, pre_read_queue: u32) {
        self.ingress_read_calls = self.ingress_read_calls.saturating_add(1);
        self.ingress_read_pre_queue_bytes_total =
            self.ingress_read_pre_queue_bytes_total.saturating_add(pre_read_queue);
        self.ingress_read_pre_queue_max = self.ingress_read_pre_queue_max.max(pre_read_queue);
        if pre_read_queue == 0 {
            self.ingress_read_pre_queue_empty_calls =
                self.ingress_read_pre_queue_empty_calls.saturating_add(1);
        }
    }

    pub(super) fn note_read_result(
        &mut self,
        pre_read_queue: u32,
        read_wait_ms: u32,
        n: usize,
        want: usize,
        ingress_adapt: &mut IngressFairnessAdaptive,
    ) {
        self.body_read_ms = self.body_read_ms.saturating_add(read_wait_ms);
        if pre_read_queue == 0 {
            self.ingress_read_wait_empty_q_ms =
                self.ingress_read_wait_empty_q_ms.saturating_add(read_wait_ms);
            self.ingress_read_wait_empty_q_max_ms =
                self.ingress_read_wait_empty_q_max_ms.max(read_wait_ms);
            self.ingress_read_empty_streak_ms =
                self.ingress_read_empty_streak_ms.saturating_add(read_wait_ms);
            self.ingress_read_empty_streak_ms_max = self
                .ingress_read_empty_streak_ms_max
                .max(self.ingress_read_empty_streak_ms);
            if read_wait_ms >= INGRESS_READ_WAIT_OVER_10MS {
                self.ingress_read_wait_empty_q_over_10ms =
                    self.ingress_read_wait_empty_q_over_10ms.saturating_add(1);
            }
            if read_wait_ms >= INGRESS_READ_WAIT_OVER_50MS {
                self.ingress_read_wait_empty_q_over_50ms =
                    self.ingress_read_wait_empty_q_over_50ms.saturating_add(1);
            }
            if read_wait_ms >= INGRESS_READ_WAIT_OVER_100MS {
                self.ingress_read_wait_empty_q_over_100ms =
                    self.ingress_read_wait_empty_q_over_100ms.saturating_add(1);
            }
        } else {
            self.ingress_read_wait_nonempty_q_ms =
                self.ingress_read_wait_nonempty_q_ms.saturating_add(read_wait_ms);
            self.ingress_read_empty_streak_ms = 0;
        }
        if read_wait_ms >= INGRESS_READ_WAIT_OVER_10MS {
            self.ingress_read_wait_over_10ms = self.ingress_read_wait_over_10ms.saturating_add(1);
        }
        if read_wait_ms >= INGRESS_READ_WAIT_OVER_50MS {
            self.ingress_read_wait_over_50ms = self.ingress_read_wait_over_50ms.saturating_add(1);
        }
        if read_wait_ms >= INGRESS_READ_WAIT_OVER_100MS {
            self.ingress_read_wait_over_100ms =
                self.ingress_read_wait_over_100ms.saturating_add(1);
        }
        ingress_adapt.observe_read(pre_read_queue == 0, read_wait_ms);
        if n < want {
            self.ingress_read_short_calls = self.ingress_read_short_calls.saturating_add(1);
        }
        self.ingress_read_ops_since_try_drain =
            self.ingress_read_ops_since_try_drain.saturating_add(1);
        if pre_read_queue > 0 {
            self.ingress_read_bytes_since_yield =
                self.ingress_read_bytes_since_yield.saturating_add(n);
            self.ingress_read_ops_since_yield =
                self.ingress_read_ops_since_yield.saturating_add(1);
        } else {
            self.ingress_read_bytes_since_yield = 0;
            self.ingress_read_ops_since_yield = 0;
        }
    }

    pub(super) fn should_yield(&self, ingress_adapt: &IngressFairnessAdaptive) -> bool {
        self.ingress_read_bytes_since_yield >= ingress_adapt.yield_bytes_target()
            || self.ingress_read_ops_since_yield >= ingress_adapt.yield_reads_target()
    }

    pub(super) fn reset_yield_counters(&mut self) {
        self.ingress_read_bytes_since_yield = 0;
        self.ingress_read_ops_since_yield = 0;
    }

    pub(super) fn record_ingress_flush_wait(&mut self, flush_ms: u32) {
        self.ingress_flush_wait_ms = self.ingress_flush_wait_ms.saturating_add(flush_ms);
    }

    pub(super) fn apply_finished_chunk(
        &mut self,
        len: usize,
        queue_ms: u32,
        copy_ms: u32,
        chunk_finish: SdUploadChunkFinish,
    ) {
        let roundtrip_ms = chunk_finish.roundtrip_ms;
        let task_wait_ms = roundtrip_ms.saturating_sub(queue_ms);
        let task_residual_ms = task_wait_ms
            .saturating_sub(chunk_finish.queue_wait_ms)
            .saturating_sub(chunk_finish.handler_ms);
        let post_handler_ms = chunk_finish.post_handler_ms.min(task_residual_ms);
        let residual_after_post_handler = task_residual_ms.saturating_sub(post_handler_ms);
        let publish_to_receive_ms = chunk_finish
            .publish_to_receive_ms
            .min(residual_after_post_handler);
        let residual_other_ms = residual_after_post_handler.saturating_sub(publish_to_receive_ms);
        self.payload_copy_ms = self.payload_copy_ms.saturating_add(copy_ms);
        self.sd_queue_ms = self.sd_queue_ms.saturating_add(queue_ms);
        self.sd_task_wait_ms = self.sd_task_wait_ms.saturating_add(task_wait_ms);
        self.sd_task_queue_wait_ms = self
            .sd_task_queue_wait_ms
            .saturating_add(chunk_finish.queue_wait_ms);
        self.sd_task_handler_ms = self.sd_task_handler_ms.saturating_add(chunk_finish.handler_ms);
        self.sd_task_residual_ms = self.sd_task_residual_ms.saturating_add(task_residual_ms);
        self.sd_task_post_handler_ms =
            self.sd_task_post_handler_ms.saturating_add(post_handler_ms);
        self.sd_task_publish_to_receive_ms = self
            .sd_task_publish_to_receive_ms
            .saturating_add(publish_to_receive_ms);
        self.sd_task_residual_other_ms = self
            .sd_task_residual_other_ms
            .saturating_add(residual_other_ms);
        self.sd_wait_ms = self.sd_wait_ms.saturating_add(roundtrip_ms);
        self.sent_bytes = self.sent_bytes.saturating_add(len);
        self.chunk_count = self.chunk_count.saturating_add(1);
        self.max_chunk_bytes = self.max_chunk_bytes.max(len);
        record_chunk_latency_sample(&mut self.chunk_samples, roundtrip_ms);
    }

    pub(super) fn finish(self, ingress_adapt_snapshot: IngressFairnessAdaptiveSnapshot) -> UploadBodyStats {
        let (chunk_p50_ms, chunk_p95_ms) = chunk_latency_quantiles(&self.chunk_samples);
        UploadBodyStats {
            sent_bytes: self.sent_bytes,
            chunk_count: self.chunk_count,
            max_chunk_bytes: self.max_chunk_bytes,
            body_read_ms: self.body_read_ms,
            payload_copy_ms: self.payload_copy_ms,
            sd_queue_ms: self.sd_queue_ms,
            sd_task_wait_ms: self.sd_task_wait_ms,
            sd_task_queue_wait_ms: self.sd_task_queue_wait_ms,
            sd_task_handler_ms: self.sd_task_handler_ms,
            sd_task_residual_ms: self.sd_task_residual_ms,
            sd_task_post_handler_ms: self.sd_task_post_handler_ms,
            sd_task_publish_to_receive_ms: self.sd_task_publish_to_receive_ms,
            sd_task_residual_other_ms: self.sd_task_residual_other_ms,
            sd_wait_ms: self.sd_wait_ms,
            chunk_p50_ms,
            chunk_p95_ms,
            chunk_max_ms: self.chunk_samples.max_ms(),
            chunk_samples: self.chunk_samples.len() as u32,
            chunk_samples_dropped: self.chunk_samples.dropped(),
            ingress_flush_wait_ms: self.ingress_flush_wait_ms,
            ingress_read_calls: self.ingress_read_calls,
            ingress_read_pre_queue_bytes_total: self.ingress_read_pre_queue_bytes_total,
            ingress_read_pre_queue_max: self.ingress_read_pre_queue_max,
            ingress_read_pre_queue_empty_calls: self.ingress_read_pre_queue_empty_calls,
            ingress_read_short_calls: self.ingress_read_short_calls,
            ingress_read_wait_empty_q_ms: self.ingress_read_wait_empty_q_ms,
            ingress_read_wait_nonempty_q_ms: self.ingress_read_wait_nonempty_q_ms,
            ingress_read_wait_over_10ms: self.ingress_read_wait_over_10ms,
            ingress_read_wait_over_50ms: self.ingress_read_wait_over_50ms,
            ingress_read_wait_over_100ms: self.ingress_read_wait_over_100ms,
            ingress_read_wait_empty_q_over_10ms: self.ingress_read_wait_empty_q_over_10ms,
            ingress_read_wait_empty_q_over_50ms: self.ingress_read_wait_empty_q_over_50ms,
            ingress_read_wait_empty_q_over_100ms: self.ingress_read_wait_empty_q_over_100ms,
            ingress_read_wait_empty_q_max_ms: self.ingress_read_wait_empty_q_max_ms,
            ingress_read_empty_streak_ms_max: self.ingress_read_empty_streak_ms_max,
            ingress_adapt_enabled: if ingress_adapt_snapshot.enabled { 1 } else { 0 },
            ingress_adapt_switches: ingress_adapt_snapshot.switches,
            ingress_adapt_level_max: ingress_adapt_snapshot.level_max as u32,
            ingress_read_empty_streak_max: ingress_adapt_snapshot.empty_streak_max,
        }
    }
}
