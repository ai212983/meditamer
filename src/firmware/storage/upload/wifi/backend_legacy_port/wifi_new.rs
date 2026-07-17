use super::{RadioController, WifiController, WifiDevice, WifiError};
use esp_hal::peripherals::WIFI;
use esp_println::println;

pub(crate) fn wifi_new_legacy(
    _radio_ctrl: &RadioController,
    wifi: WIFI<'static>,
    config: esp_radio::wifi::Config,
) -> Result<
    (
        WifiController<'static>,
        esp_radio::wifi::Interfaces<'static>,
    ),
    WifiError,
> {
    println!("upload_http: legacy_port wifi_new stage=begin");
    let result = esp_radio::wifi::backend_legacy_port_wifi_new(wifi, config);
    match &result {
        Ok(_) => println!("upload_http: legacy_port wifi_new stage=done"),
        Err(err) => println!("upload_http: legacy_port wifi_new err={err:?}"),
    }
    result
}
