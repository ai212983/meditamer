use super::super::{
    backend_legacy_port, WifiController, WifiError,
};
use super::LegacyDiscoverySession;

pub(crate) async fn begin_session<'a>(
    controller: &'a mut WifiController<'static>,
) -> Result<LegacyDiscoverySession<'a>, WifiError> {
    backend_legacy_port::controller_set_mode(controller, backend_legacy_port::legacy_sta_mode())?;
    let owns_start = if backend_legacy_port::controller_is_started(controller)? {
        false
    } else {
        backend_legacy_port::controller_start(controller).await?;
        true
    };

    Ok(LegacyDiscoverySession {
        controller,
        owns_start,
    })
}
