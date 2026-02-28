#[cfg(feature = "asset-upload-http")]
use core::sync::atomic::{AtomicBool, Ordering};

use crate::firmware::app_state::{self, Phase};

#[cfg(feature = "asset-upload-http")]
static UPLOAD_HTTP_LISTENER_ENABLED: AtomicBool = AtomicBool::new(true);

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
pub(crate) fn set_upload_http_listener_enabled(enabled: bool) {
    UPLOAD_HTTP_LISTENER_ENABLED.store(enabled, Ordering::Relaxed);
}
