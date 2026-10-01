#[cfg(feature = "asset-upload-http")]
use crate::firmware::app_state::{self, Phase};

// Listener enable/sequence state and radio-handoff admission moved to
// `netstack::host` in the product/target axis completion plan's Phase 4:
// they are network policy and operational state the HTTP service reads
// inward through the platform API, not product state the network owner
// polls back out. Only Meditamer's own upload-enabled policy stays here.

#[cfg(feature = "asset-upload-http")]
pub fn upload_enabled() -> bool {
    app_state::snapshot::upload_enabled()
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_transfers_enabled() -> bool {
    let snapshot = app_state::snapshot::read_app_state_snapshot();
    snapshot.services.upload_enabled && !matches!(snapshot.phase, Phase::DiagnosticsExclusive)
}
