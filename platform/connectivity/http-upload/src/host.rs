//! Product ports for the shared streaming request engine. Implementations keep
//! storage ownership and telemetry in their existing tasks. Chunk tokens must
//! retain exclusive access to staged bytes until finish or try_finish succeeds.
use sdcard::SD_PATH_MAX;

pub enum Command {
    Begin {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        expected_size: u32,
    },
    Commit,
    Abort,
    Mkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Remove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Stat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
}

pub struct UploadHttpPhaseMetrics {
    pub bytes: u32,
    pub body_read_ms: u32,
    pub payload_copy_ms: u32,
    pub sd_queue_ms: u32,
    pub sd_task_wait_ms: u32,
    pub commit_ms: u32,
    pub chunk_p50_ms: u32,
    pub chunk_p95_ms: u32,
    pub chunk_max_ms: u32,
    pub chunk_samples: u32,
    pub chunk_samples_dropped: u32,
    pub sd_wait_ms: u32,
    pub request_ms: u32,
}

#[derive(Clone, Copy)]
pub enum LogDomain {
    Http,
    Sd,
}
#[derive(Default)]
pub struct RssiSnapshot {
    pub wifi_link_rssi_last_dbm: i32,
    pub wifi_link_rssi_min_dbm: i32,
    pub wifi_link_rssi_max_dbm: i32,
    pub wifi_link_rssi_samples: u32,
    pub wifi_link_rssi_low_samples: u32,
}
pub struct ChunkStarted<I> {
    pub transfer: I,
    pub copy_ms: u32,
}
pub enum ChunkTryFinish<I> {
    Pending(I),
    Finished(ChunkFinish),
}
pub struct ChunkFinish {
    pub roundtrip_ms: u32,
    pub queue_wait_ms: u32,
    pub handler_ms: u32,
    pub post_handler_ms: u32,
    pub publish_to_receive_ms: u32,
}

#[allow(async_fn_in_trait)]
pub trait Host {
    type Error: Copy;
    type Inflight;
    const HTTP_INGRESS_ADAPTIVE_FAIRNESS: bool = false;
    const HTTP_INGRESS_COOP_YIELD_BYTES: usize = 16 * 1024;
    const HTTP_INGRESS_COOP_YIELD_READS: u32 = 32;
    const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS: u32 = 2;
    fn admission_open() -> bool;
    fn upload_token() -> Option<&'static [u8]>;
    async fn roundtrip(command: Command) -> Result<(), Self::Error>;
    async fn chunk_start(data: &[u8]) -> Result<ChunkStarted<Self::Inflight>, Self::Error>;
    async fn chunk_finish(transfer: Self::Inflight) -> Result<ChunkFinish, Self::Error>;
    fn chunk_try_finish(
        transfer: Self::Inflight,
    ) -> Result<ChunkTryFinish<Self::Inflight>, Self::Error>;
    fn error_log(error: Self::Error) -> &'static str;
    fn error_status(error: Self::Error) -> &'static [u8];
    fn error_body(error: Self::Error) -> &'static [u8];
    fn log_filter_enabled(_domain: LogDomain) -> bool {
        false
    }
    fn log(_message: core::fmt::Arguments<'_>) {}
    fn log_stack_headroom(_tag: &'static str) {}
    fn record_upload_http_health_request() {}
    fn record_upload_http_read_body_reset() {}
    fn record_upload_http_upload_phase(_metrics: UploadHttpPhaseMetrics) {}
    fn snapshot() -> RssiSnapshot {
        RssiSnapshot::default()
    }
}
