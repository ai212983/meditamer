#[cfg(feature = "asset-upload-http")]
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(feature = "asset-upload-http")]
use crate::firmware::app_state::{self, Phase};

#[cfg(feature = "asset-upload-http")]
static UPLOAD_HTTP_LISTENER_ENABLED: AtomicBool = AtomicBool::new(true);
#[cfg(feature = "asset-upload-http")]
static UPLOAD_HTTP_LISTENER_SET_SEQ: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "asset-upload-http")]
static RADIO_HANDOFF_ADMISSION_OPEN: AtomicBool = AtomicBool::new(true);

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_enabled() -> bool {
    app_state::snapshot::upload_enabled()
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_transfers_enabled() -> bool {
    let snapshot = app_state::snapshot::read_app_state_snapshot();
    snapshot.services.upload_enabled && !matches!(snapshot.phase, Phase::DiagnosticsExclusive)
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_http_listener_enabled() -> bool {
    UPLOAD_HTTP_LISTENER_ENABLED.load(Ordering::Relaxed)
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_http_listener_set_seq() -> u32 {
    UPLOAD_HTTP_LISTENER_SET_SEQ.load(Ordering::Relaxed)
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn set_upload_http_listener_enabled(enabled: bool) {
    UPLOAD_HTTP_LISTENER_ENABLED.store(enabled, Ordering::Relaxed);
    UPLOAD_HTTP_LISTENER_SET_SEQ.fetch_add(1, Ordering::Relaxed);
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn radio_handoff_admission_open() -> bool {
    RADIO_HANDOFF_ADMISSION_OPEN.load(Ordering::Acquire)
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn set_radio_handoff_admission_open(open: bool) {
    RADIO_HANDOFF_ADMISSION_OPEN.store(open, Ordering::Release);
}
