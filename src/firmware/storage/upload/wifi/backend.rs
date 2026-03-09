#[cfg(feature = "wifi-backend-esp-radio")]
mod esp_radio_backend {
    extern crate alloc;

    use crate::firmware::storage::upload::wifi::backend_legacy_port;
    use embassy_time::Duration;
    use enumset::EnumSet;
    use esp_radio::wifi::Interfaces;
    use esp_radio::InitializationError;

    pub(crate) type RadioController = esp_radio::Controller<'static>;
    pub(crate) type WifiError = esp_radio::wifi::WifiError;
    pub(crate) type WifiDriverConfig = esp_radio::wifi::Config;
    pub(crate) type WifiController<'a> = esp_radio::wifi::WifiController<'a>;
    pub(crate) type WifiDevice<'a> = esp_radio::wifi::WifiDevice<'a>;
    pub(crate) type WifiInterfaces<'a> = Interfaces<'a>;

    pub(crate) use esp_radio::wifi::event::{self, EventExt};
    pub(crate) use esp_radio::wifi::{
        AccessPointInfo, AuthMethod, ClientConfig, ModeConfig, PowerSaveMode, Protocol, ScanConfig,
        ScanMethod, ScanTypeConfig, WifiMode,
    };

    pub(crate) const NAME: &str = "esp-radio";

    pub(crate) fn init_radio() -> Result<RadioController, InitializationError> {
        esp_radio::init()
    }

    pub(crate) fn wifi_runtime_config(country_us_override: bool) -> WifiDriverConfig {
        if country_us_override {
            WifiDriverConfig::default()
                .with_country_code(esp_radio::wifi::CountryInfo::from(*b"US"))
        } else {
            WifiDriverConfig::default()
        }
    }

    pub(crate) fn new_runtime(
        radio: &'static RadioController,
        wifi: esp_hal::peripherals::WIFI<'static>,
        config: WifiDriverConfig,
    ) -> Result<(WifiController<'static>, WifiInterfaces<'static>), WifiError> {
        esp_radio::wifi::new(radio, wifi, config)
    }

    pub(crate) fn wifi_set_config(
        controller: &mut WifiController<'_>,
        conf: &ModeConfig,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::set_config(controller, conf)
    }

    pub(crate) fn wifi_set_mode(
        controller: &mut WifiController<'_>,
        mode: WifiMode,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::set_mode(controller, mode)
    }

    pub(crate) fn wifi_is_started(controller: &WifiController<'_>) -> Result<bool, WifiError> {
        esp_radio::wifi::WifiController::is_started(controller)
    }

    pub(crate) fn wifi_set_power_saving(
        controller: &mut WifiController<'_>,
        ps: PowerSaveMode,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::set_power_saving(controller, ps)
    }

    pub(crate) fn wifi_set_protocol(
        controller: &mut WifiController<'_>,
        protocols: EnumSet<Protocol>,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::set_protocol(controller, protocols)
    }

    pub(crate) fn wifi_sta_mode() -> WifiMode {
        WifiMode::Sta
    }

    pub(crate) fn wifi_power_save_none() -> PowerSaveMode {
        PowerSaveMode::None
    }

    pub(crate) fn wifi_standard_bgn_protocols() -> EnumSet<Protocol> {
        Protocol::P802D11B | Protocol::P802D11BG | Protocol::P802D11BGN
    }

    pub(crate) fn wifi_rssi(controller: &WifiController<'_>) -> Result<i32, WifiError> {
        esp_radio::wifi::WifiController::rssi(controller)
    }

    pub(crate) fn wifi_error_is_no_mem(err: &WifiError) -> bool {
        matches!(
            err,
            WifiError::InternalError(esp_radio::wifi::InternalWifiError::NoMem)
        )
    }

    pub(crate) fn wifi_client_mode_config(
        ssid: &str,
        password: &str,
        auth_method: AuthMethod,
        channel_hint: Option<u8>,
        bssid_hint: Option<[u8; 6]>,
    ) -> ModeConfig {
        let auth_method = if password.is_empty() {
            AuthMethod::None
        } else {
            auth_method
        };
        let scan_method = if channel_hint.is_some() {
            ScanMethod::Fast
        } else {
            ScanMethod::AllChannels
        };
        let mut client = ClientConfig::default()
            .with_ssid(ssid.into())
            .with_password(password.into())
            .with_auth_method(auth_method)
            .with_scan_method(scan_method);
        if let Some(channel) = channel_hint {
            client = client.with_channel(channel);
        }
        if let Some(bssid) = bssid_hint {
            client = client.with_bssid(bssid);
        }
        ModeConfig::Client(client)
    }

    pub(crate) fn wifi_active_scan_config(
        max_results: usize,
        min_ms: u64,
        max_ms: u64,
    ) -> ScanConfig<'static> {
        ScanConfig::default()
            .with_show_hidden(true)
            .with_max(max_results)
            .with_scan_type(ScanTypeConfig::Active {
                min: Duration::from_millis(min_ms).into(),
                max: Duration::from_millis(max_ms).into(),
            })
    }

    pub(crate) fn wifi_directed_active_scan_config(
        ssid: &str,
        max_results: usize,
        min_ms: u64,
        max_ms: u64,
    ) -> ScanConfig<'_> {
        wifi_active_scan_config(max_results, min_ms, max_ms).with_ssid(ssid)
    }

    pub(crate) fn wifi_channel_active_scan_config(
        channel: u8,
        max_results: usize,
        min_ms: u64,
        max_ms: u64,
    ) -> ScanConfig<'static> {
        wifi_active_scan_config(max_results, min_ms, max_ms).with_channel(channel)
    }

    pub(crate) fn wifi_passive_scan_config(
        max_results: usize,
        passive_ms: u64,
    ) -> ScanConfig<'static> {
        ScanConfig::default()
            .with_show_hidden(true)
            .with_max(max_results)
            .with_scan_type(ScanTypeConfig::Passive(
                Duration::from_millis(passive_ms).into(),
            ))
    }

    pub(crate) fn wifi_raw_broad_scan_config(max_results: usize) -> ScanConfig<'static> {
        ScanConfig::default()
            .with_show_hidden(true)
            .with_max(max_results)
    }

    pub(crate) async fn wifi_scan_with_config_async(
        controller: &mut WifiController<'_>,
        config: ScanConfig<'_>,
    ) -> Result<self::alloc::vec::Vec<AccessPointInfo>, WifiError> {
        if backend_legacy_port::legacy_port_runtime_enabled() {
            backend_legacy_port::controller_scan_with_config(controller, config).await
        } else {
            esp_radio::wifi::WifiController::scan_with_config_async(controller, config).await
        }
    }

    pub(crate) async fn wifi_start_async(
        controller: &mut WifiController<'_>,
    ) -> Result<(), WifiError> {
        if backend_legacy_port::legacy_port_runtime_enabled() {
            backend_legacy_port::controller_start(controller).await
        } else {
            esp_radio::wifi::WifiController::start_async(controller).await
        }
    }

    pub(crate) async fn wifi_stop_async(
        controller: &mut WifiController<'_>,
    ) -> Result<(), WifiError> {
        if backend_legacy_port::legacy_port_runtime_enabled() {
            backend_legacy_port::controller_stop(controller).await
        } else {
            esp_radio::wifi::WifiController::stop_async(controller).await
        }
    }

    pub(crate) async fn wifi_connect_async(
        controller: &mut WifiController<'_>,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::connect_async(controller).await
    }

    pub(crate) async fn wifi_disconnect_async(
        controller: &mut WifiController<'_>,
    ) -> Result<(), WifiError> {
        esp_radio::wifi::WifiController::disconnect_async(controller).await
    }
}

#[cfg(feature = "wifi-backend-esp-radio")]
pub(crate) use esp_radio_backend::*;
