use crate::firmware::app_state::{self, Phase};

pub(crate) fn upload_enabled() -> bool {
    app_state::snapshot::upload_enabled()
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_transfers_enabled() -> bool {
    let snapshot = app_state::snapshot::read_app_state_snapshot();
    snapshot.services.upload_enabled && !matches!(snapshot.phase, Phase::DiagnosticsExclusive)
}
