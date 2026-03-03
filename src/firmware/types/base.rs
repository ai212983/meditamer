use crate::{
    drivers::inkplate::InkplateHal,
    drivers::platform::{BusyDelay, HalI2c},
};
use esp_hal::{gpio::Output, uart::Uart, Async};
use sdcard::probe;

use super::super::app_state::AppStateStore;

pub(crate) type InkplateDriver = InkplateHal<HalI2c<'static>, BusyDelay>;
pub(crate) type SerialUart = Uart<'static, Async>;
pub(crate) type SdProbeDriver = probe::SdCardProbe<'static>;
pub(crate) use sdcard::{SD_PATH_MAX, SD_WRITE_MAX};
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const SD_UPLOAD_CHUNK_MAX_DEFAULT: usize = 65_536;
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const SD_UPLOAD_CHUNK_MAX_MIN: usize = 4_096;
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const SD_UPLOAD_CHUNK_MAX_MAX: usize = 65_536;
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const fn parse_ascii_usize(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut out = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b < b'0' || b > b'9' {
            return None;
        }
        let digit = (b - b'0') as usize;
        let multiplied = match out.checked_mul(10) {
            Some(v) => v,
            None => return None,
        };
        out = match multiplied.checked_add(digit) {
            Some(v) => v,
            None => return None,
        };
        i += 1;
    }
    Some(out)
}
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const fn configured_sd_upload_chunk_max() -> usize {
    let configured = match option_env!("MEDITAMER_SD_UPLOAD_CHUNK_MAX") {
        Some(v) => Some(v),
        None => option_env!("SD_UPLOAD_CHUNK_MAX"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(bytes) if bytes >= SD_UPLOAD_CHUNK_MAX_MIN && bytes <= SD_UPLOAD_CHUNK_MAX_MAX => {
            bytes
        }
        _ => SD_UPLOAD_CHUNK_MAX_DEFAULT,
    }
}
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
// Larger upload chunks reduce per-chunk SD roundtrip overhead and improve
// sustained HTTP upload throughput when PSRAM is available.
// Override at build time via MEDITAMER_SD_UPLOAD_CHUNK_MAX (fallback SD_UPLOAD_CHUNK_MAX).
pub(crate) const SD_UPLOAD_CHUNK_MAX: usize = configured_sd_upload_chunk_max();
#[cfg(all(feature = "asset-upload-http", not(feature = "psram-alloc")))]
pub(crate) const SD_UPLOAD_CHUNK_MAX: usize = 4096;
#[cfg(not(feature = "asset-upload-http"))]
pub(crate) const SD_UPLOAD_CHUNK_MAX: usize = 1024;
#[cfg(feature = "asset-upload-http")]
pub(crate) const SD_ASSET_READ_MAX: usize = 1024;
#[cfg(not(feature = "asset-upload-http"))]
pub(crate) const SD_ASSET_READ_MAX: usize = 3072;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_SSID_MAX: usize = 32;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_PASSWORD_MAX: usize = 64;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_CONFIG_FILE_MAX: usize = 192;

pub(crate) struct DisplayContext {
    pub(crate) inkplate: InkplateDriver,
    pub(crate) app_state_store: AppStateStore<'static>,
    pub(crate) _panel_pins: PanelPinHold<'static>,
}

pub(crate) struct PanelPinHold<'d> {
    pub(crate) _cl: Output<'d>,
    pub(crate) _le: Output<'d>,
    pub(crate) _d0: Output<'d>,
    pub(crate) _d1: Output<'d>,
    pub(crate) _d2: Output<'d>,
    pub(crate) _d3: Output<'d>,
    pub(crate) _d4: Output<'d>,
    pub(crate) _d5: Output<'d>,
    pub(crate) _d6: Output<'d>,
    pub(crate) _d7: Output<'d>,
    pub(crate) _ckv: Output<'d>,
    pub(crate) _sph: Output<'d>,
}
