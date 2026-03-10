use super::{backend_esp_radio, RadioController, WifiController, WifiDevice};
use crate::firmware::storage::upload::wifi::backend_legacy_port;
use esp_radio::InitializationError;

pub(crate) const NAME: &str = "esp-radio";

pub(crate) fn legacy_port_runtime_enabled() -> bool {
    backend_legacy_port::legacy_port_runtime_enabled()
}

pub(crate) fn backend_name() -> &'static str {
    if legacy_port_runtime_enabled() {
        backend_legacy_port::LEGACY_RUNTIME_NAME
    } else {
        NAME
    }
}

pub(crate) fn init_radio() -> Result<RadioController, InitializationError> {
    backend_esp_radio::init_radio()
}

pub(crate) fn wifi_runtime_config(country_us_override: bool) -> super::WifiDriverConfig {
    if legacy_port_runtime_enabled() {
        backend_legacy_port::legacy_runtime_config(country_us_override)
    } else {
        backend_esp_radio::wifi_runtime_config(country_us_override)
    }
}

pub(crate) fn new_runtime(
    radio: &'static RadioController,
    wifi: esp_hal::peripherals::WIFI<'static>,
    config: super::WifiDriverConfig,
) -> Result<
    (
        WifiController<'static>,
        backend_esp_radio::WifiInterfaces<'static>,
    ),
    super::WifiError,
> {
    backend_esp_radio::new_runtime(radio, wifi, config)
}

pub(crate) fn initialize_runtime_sta(
    wifi: esp_hal::peripherals::WIFI<'static>,
    country_us_override: bool,
) -> Result<(WifiController<'static>, WifiDevice<'static>), &'static str> {
    if legacy_port_runtime_enabled() {
        backend_legacy_port::initialize_runtime_sta_legacy_port(wifi, country_us_override)
    } else {
        backend_esp_radio::initialize_runtime_sta(wifi, country_us_override)
    }
}
