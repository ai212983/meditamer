use crate::firmware::types::{InkplateDriver, TimeSyncState};

pub(crate) fn next_visual_seed(
    _uptime_seconds: u32,
    _time_sync: Option<TimeSyncState>,
    pattern_nonce: &mut u32,
    first_visual_seed_pending: &mut bool,
) -> u32 {
    if *first_visual_seed_pending {
        *first_visual_seed_pending = false;
    }
    *pattern_nonce = pattern_nonce.wrapping_add(1);
    *pattern_nonce
}

pub(crate) async fn render_suminagashi_update(
    _display: &mut InkplateDriver,
    _seed: u32,
    _uptime_seconds: u32,
    _time_sync: Option<TimeSyncState>,
) {
}

pub(crate) async fn render_shanshui_update(
    _display: &mut InkplateDriver,
    _seed: u32,
    _uptime_seconds: u32,
    _time_sync: Option<TimeSyncState>,
) {
}
