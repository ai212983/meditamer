use super::super::{
    wifi_is_started, wifi_set_mode, wifi_sta_mode, wifi_start_async, WifiController, WifiError,
};
use super::LegacyDiscoverySession;

pub(crate) async fn begin_session<'a>(
    controller: &'a mut WifiController<'static>,
) -> Result<LegacyDiscoverySession<'a>, WifiError> {
    wifi_set_mode(controller, wifi_sta_mode())?;
    let owns_start = if wifi_is_started(controller)? {
        false
    } else {
        wifi_start_async(controller).await?;
        true
    };

    Ok(LegacyDiscoverySession {
        controller,
        owns_start,
    })
}
