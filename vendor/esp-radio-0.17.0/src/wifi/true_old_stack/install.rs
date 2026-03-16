use esp_wifi_sys::include::{wifi_init_config_t, wpa_crypto_funcs_t};

use super::super::Config;

pub(crate) unsafe fn install_globals(config: Config, wpa_crypto: wpa_crypto_funcs_t) {
    unsafe { super::super::legacy_stack::install::install_legacy_literal_g_config(config, wpa_crypto) };
}

#[cfg(all(coex, any(esp32, esp32c2, esp32c3, esp32c6, esp32s3)))]
pub(crate) fn coex_adapter_funcs() -> *const crate::binary::include::coex_adapter_funcs_t {
    &raw const super::super::legacy_stack::install::G_COEX_ADAPTER_FUNCS
}

pub(crate) fn wifi_init_config() -> *const wifi_init_config_t {
    &raw const super::super::legacy_stack::install::G_CONFIG
}
