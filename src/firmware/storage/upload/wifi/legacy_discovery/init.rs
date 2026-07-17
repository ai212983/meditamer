use super::super::{WifiController, WifiError};
use super::LegacyDiscoverySession;

pub(crate) async fn begin_session<'a>(
    controller: &'a mut WifiController<'static>,
) -> Result<LegacyDiscoverySession<'a>, WifiError> {
    Ok(LegacyDiscoverySession {
        controller,
        owns_start: false,
    })
}
