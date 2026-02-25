use super::{WIFI_PASSWORD_MAX, WIFI_SSID_MAX};

#[cfg(feature = "asset-upload-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WifiCredentials {
    pub(crate) ssid: [u8; WIFI_SSID_MAX],
    pub(crate) ssid_len: u8,
    pub(crate) password: [u8; WIFI_PASSWORD_MAX],
    pub(crate) password_len: u8,
}

#[cfg(feature = "asset-upload-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WifiConfigRequest {
    Load,
    Store { credentials: WifiCredentials },
}

#[cfg(feature = "asset-upload-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WifiConfigResultCode {
    Ok,
    Busy,
    NotFound,
    InvalidData,
    PowerOnFailed,
    InitFailed,
    OperationFailed,
}

#[cfg(feature = "asset-upload-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WifiConfigResponse {
    pub(crate) ok: bool,
    pub(crate) code: WifiConfigResultCode,
    pub(crate) credentials: Option<WifiCredentials>,
}
