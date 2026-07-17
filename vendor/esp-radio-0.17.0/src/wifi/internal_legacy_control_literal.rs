use alloc::vec::Vec;

use crate::binary::include::{self, wifi_interface_t_WIFI_IF_AP, wifi_interface_t_WIFI_IF_STA};

use super::{
    AccessPointInfo, ScanConfig, WifiController, WifiError, WifiMode, esp_wifi_result,
    internal_legacy_scan_literal,
};

pub(crate) fn start(controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    unsafe {
        esp_wifi_result!(include::esp_wifi_start())?;

        let mode = WifiMode::current()?;

        if mode.is_ap() {
            esp_wifi_result!(include::esp_wifi_set_inactive_time(
                wifi_interface_t_WIFI_IF_AP,
                controller.ap_beacon_timeout,
            ))?;
        }
        if mode.is_sta() {
            esp_wifi_result!(include::esp_wifi_set_inactive_time(
                wifi_interface_t_WIFI_IF_STA,
                controller.beacon_timeout,
            ))?;
        }
    }

    Ok(())
}

pub(crate) fn stop(_controller: &mut WifiController<'_>) -> Result<(), WifiError> {
    esp_wifi_result!(unsafe { include::esp_wifi_stop() })
}

pub(crate) fn scan_with_config(
    _controller: &mut WifiController<'_>,
    config: ScanConfig<'_>,
) -> Result<Vec<AccessPointInfo>, WifiError> {
    let max = config.max.unwrap_or(usize::MAX);
    let use_legacy_scan_n = matches!(
        config,
        ScanConfig {
            ssid: None,
            bssid: None,
            channel: None,
            show_hidden: false,
            scan_type: super::ScanTypeConfig::Active { .. },
            ..
        }
    );
    if use_legacy_scan_n {
        return scan_n(max);
    }

    internal_legacy_scan_literal::scan_with_config_sync_max(config, max)
}

pub(crate) fn scan_n(max: usize) -> Result<Vec<AccessPointInfo>, WifiError> {
    internal_legacy_scan_literal::scan_n(max)
}
