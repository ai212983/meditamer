use super::*;

pub fn mode_config_from_credentials(
    credentials: WifiCredentials,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
    bssid_hint: Option<[u8; 6]>,
) -> Option<ModeConfig> {
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).ok()?;
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).ok()?;
    wifi_client_mode_config(ssid, password, auth_method, channel_hint, bssid_hint)
}
