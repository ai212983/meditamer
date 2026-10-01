//! Observability counters.
//!
//! Plain atomics: [`super::recorders`] writes them, [`super::snapshot`] reads
//! them. Nothing here interprets a value.
//!
//! Wi-Fi connect/reassociation, link, IPv4, listener, and network-pipeline
//! counters moved to `platform/connectivity/netstack::telemetry` in the product/target
//! axis completion plan's Phase 4; this module keeps the product's own
//! upload-service and SD-roundtrip timing.

use core::sync::atomic::AtomicU32;

pub(super) static UPLOAD_HTTP_ACCEPTS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_ACCEPT_ERRORS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_ACCEPT_LINK_RESETS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_REQUEST_ERRORS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_HEADER_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_READ_BODY_ERRORS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_READ_BODY_RESETS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_SD_BUSY_ERRORS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_HEALTH_REQUESTS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_REQUESTS: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_BYTES: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_PAYLOAD_COPY_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_QUEUE_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_TASK_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_COMMIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_COMMIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_CHUNK_P50_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_CHUNK_P95_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_CHUNK_MAX_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_CHUNK_SAMPLES_DROPPED: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_ERRORS: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_BUSY: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_POWER_ON_FAILED: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_INIT_FAILED: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_SESSION_TIMEOUT_ABORTS: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_SESSION_MODE_OFF_ABORTS: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_BEGIN_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_BEGIN_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_BEGIN_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_CHUNK_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_CHUNK_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_CHUNK_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_COMMIT_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_COMMIT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_COMMIT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_ABORT_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_ABORT_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_ABORT_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_MKDIR_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_MKDIR_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_MKDIR_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_REMOVE_COUNT: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_REMOVE_MS_TOTAL: AtomicU32 = AtomicU32::new(0);
pub(super) static SD_UPLOAD_RTT_REMOVE_MS_MAX: AtomicU32 = AtomicU32::new(0);
pub(super) static BOOT_RESET_REASON_CODE: AtomicU32 = AtomicU32::new(0);
