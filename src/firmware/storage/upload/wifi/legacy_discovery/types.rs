extern crate alloc;

use super::super::{AccessPointInfo, WifiController};

pub(crate) type LegacyDiscoveryResult = alloc::vec::Vec<AccessPointInfo>;

// This wraps the current controller shape behind the future legacy-discovery seam.
pub(crate) struct LegacyDiscoverySession<'a> {
    pub(super) controller: &'a mut WifiController<'static>,
    pub(super) owns_start: bool,
}
