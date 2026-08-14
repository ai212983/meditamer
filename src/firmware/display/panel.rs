//! Panel ownership: refresh selection, power lease, dirty-refresh tracking,
//! and SD-power servicing. A sibling of [`super::presentation`]; both run
//! inside the single display task.

pub(super) mod lease;
pub(super) mod refresh;
pub(super) mod refresh_tracking;
pub(super) mod sd_power;
