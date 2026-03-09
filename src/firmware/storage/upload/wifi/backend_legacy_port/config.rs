use crate::firmware::storage::upload::wifi::backend::WifiDriverConfig;

pub(crate) fn runtime_config(country_us_override: bool) -> WifiDriverConfig {
    if country_us_override {
        WifiDriverConfig::default().with_country_code(esp_radio::wifi::CountryInfo::from(*b"US"))
    } else {
        WifiDriverConfig::default()
    }
}
