//! Product bindings for the shared request/body/pipeline engine.
use super::super::sd_bridge;
use crate::firmware::{observability, types};
use embassy_net::tcp::TcpSocket;
pub(super) use http_upload::{
    HandledRequest, RequestRouteKind, HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS,
    HTTP_HEADER_READ_TIMEOUT_MS,
};

pub(super) async fn handle_connection(
    socket: &mut TcpSocket<'_>,
    chunk: &mut [u8],
    header: &mut [u8],
    timeout_ms: u64,
) -> Result<HandledRequest, &'static str> {
    http_upload::handle_connection::<ProductHost>(socket, chunk, header, timeout_ms).await
}
struct ProductHost;
impl http_upload::Host for ProductHost {
    type Error = sd_bridge::SdUploadRoundtripError;
    type Inflight = sd_bridge::SdUploadChunkInFlight;
    const HTTP_INGRESS_ADAPTIVE_FAIRNESS: bool = types::HTTP_INGRESS_ADAPTIVE_FAIRNESS;
    const HTTP_INGRESS_COOP_YIELD_BYTES: usize = types::HTTP_INGRESS_COOP_YIELD_BYTES;
    const HTTP_INGRESS_COOP_YIELD_READS: u32 = types::HTTP_INGRESS_COOP_YIELD_READS;
    const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS: u32 = types::HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS;
    fn admission_open() -> bool {
        netstack::host::radio_handoff_admission_open()
    }
    fn upload_token() -> Option<&'static [u8]> {
        option_env!("MEDITAMER_UPLOAD_HTTP_TOKEN")
            .or(option_env!("UPLOAD_HTTP_TOKEN"))
            .map(str::as_bytes)
    }
    async fn roundtrip(command: http_upload::Command) -> Result<(), Self::Error> {
        use http_upload::Command;
        let command = match command {
            Command::Begin {
                path,
                path_len,
                expected_size,
            } => types::SdUploadCommand::Begin {
                path,
                path_len,
                expected_size,
            },
            Command::Mkdir { path, path_len } => types::SdUploadCommand::Mkdir { path, path_len },
            Command::Remove { path, path_len } => types::SdUploadCommand::Remove { path, path_len },
            Command::Stat { path, path_len } => types::SdUploadCommand::Stat { path, path_len },
            Command::Commit => types::SdUploadCommand::Commit,
            Command::Abort => types::SdUploadCommand::Abort,
        };
        sd_bridge::sd_upload_roundtrip(command).await.map(|_| ())
    }
    async fn chunk_start(
        data: &[u8],
    ) -> Result<http_upload::ChunkStarted<Self::Inflight>, Self::Error> {
        let transfer = sd_bridge::sd_upload_chunk_start(data).await?;
        Ok(http_upload::ChunkStarted {
            copy_ms: transfer.copy_ms,
            transfer,
        })
    }
    async fn chunk_finish(
        transfer: Self::Inflight,
    ) -> Result<http_upload::ChunkFinish, Self::Error> {
        sd_bridge::sd_upload_chunk_finish(transfer)
            .await
            .map(chunk_finish)
    }
    fn chunk_try_finish(
        transfer: Self::Inflight,
    ) -> Result<http_upload::ChunkTryFinish<Self::Inflight>, Self::Error> {
        sd_bridge::sd_upload_chunk_try_finish(transfer).map(|result| match result {
            sd_bridge::SdUploadChunkTryFinish::Pending(transfer) => {
                http_upload::ChunkTryFinish::Pending(transfer)
            }
            sd_bridge::SdUploadChunkTryFinish::Finished(finish) => {
                http_upload::ChunkTryFinish::Finished(chunk_finish(finish))
            }
        })
    }
    fn error_log(error: Self::Error) -> &'static str {
        sd_bridge::roundtrip_error_log(error)
    }
    fn error_body(error: Self::Error) -> &'static [u8] {
        sd_bridge::roundtrip_error_body(error)
    }
    fn error_status(error: Self::Error) -> &'static [u8] {
        sd_bridge::roundtrip_error_status(error)
    }
    fn log_filter_enabled(domain: http_upload::LogDomain) -> bool {
        observability::log_filter_enabled(match domain {
            http_upload::LogDomain::Http => observability::LOG_DOMAIN_HTTP,
            http_upload::LogDomain::Sd => observability::LOG_DOMAIN_SD,
        })
    }
    fn log(message: core::fmt::Arguments<'_>) {
        console::println!("{}", message);
    }
    fn log_stack_headroom(tag: &'static str) {
        observability::log_stack_headroom(tag);
    }
    fn record_upload_http_health_request() {
        observability::record_upload_http_health_request();
    }
    fn record_upload_http_read_body_reset() {
        observability::record_upload_http_read_body_reset();
    }
    fn record_upload_http_upload_phase(metrics: http_upload::UploadHttpPhaseMetrics) {
        observability::record_upload_http_upload_phase(observability::UploadHttpPhaseMetrics {
            bytes: metrics.bytes,
            body_read_ms: metrics.body_read_ms,
            payload_copy_ms: metrics.payload_copy_ms,
            sd_queue_ms: metrics.sd_queue_ms,
            sd_task_wait_ms: metrics.sd_task_wait_ms,
            commit_ms: metrics.commit_ms,
            chunk_p50_ms: metrics.chunk_p50_ms,
            chunk_p95_ms: metrics.chunk_p95_ms,
            chunk_max_ms: metrics.chunk_max_ms,
            chunk_samples: metrics.chunk_samples,
            chunk_samples_dropped: metrics.chunk_samples_dropped,
            sd_wait_ms: metrics.sd_wait_ms,
            request_ms: metrics.request_ms,
        });
    }
    fn snapshot() -> http_upload::RssiSnapshot {
        let snapshot = observability::snapshot();
        http_upload::RssiSnapshot {
            wifi_link_rssi_last_dbm: snapshot.wifi_link_rssi_last_dbm,
            wifi_link_rssi_min_dbm: snapshot.wifi_link_rssi_min_dbm,
            wifi_link_rssi_max_dbm: snapshot.wifi_link_rssi_max_dbm,
            wifi_link_rssi_samples: snapshot.wifi_link_rssi_samples,
            wifi_link_rssi_low_samples: snapshot.wifi_link_rssi_low_samples,
        }
    }
}
fn chunk_finish(value: sd_bridge::SdUploadChunkFinish) -> http_upload::ChunkFinish {
    http_upload::ChunkFinish {
        roundtrip_ms: value.roundtrip_ms,
        queue_wait_ms: value.queue_wait_ms,
        handler_ms: value.handler_ms,
        post_handler_ms: value.post_handler_ms,
        publish_to_receive_ms: value.publish_to_receive_ms,
    }
}
