use crate::{
    drivers::inkplate::{imu::InkplateImu, touch::InkplateTouch, InkplateHal},
    drivers::platform::BusyDelay,
};
use embassy_embedded_hal::{adapter::BlockingAsync, shared_bus::asynch::i2c::I2cDevice};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use esp_hal::{gpio::Output, i2c::master::I2c, uart::Uart, Async, Blocking};
use sdcard::probe;

use super::super::app_state::AppStateStore;

pub(crate) type SharedI2cDevice =
    I2cDevice<'static, CriticalSectionRawMutex, BlockingAsync<I2c<'static, Blocking>>>;
pub(crate) type InkplateDriver = InkplateHal<SharedI2cDevice, BusyDelay>;
pub(crate) type InkplateImuDriver = InkplateImu<SharedI2cDevice>;
pub(crate) type InkplateTouchDriver = InkplateTouch<SharedI2cDevice>;
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
const HTTP_RX_BUF_TARGET_DEFAULT: usize = 65_536;
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const HTTP_RX_BUF_TARGET_MIN: usize = 8_192;
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
const HTTP_RX_BUF_TARGET_MAX: usize = 262_144;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT: usize = 16 * 1024;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_BYTES_MIN: usize = 1024;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_BYTES_MAX: usize = 64 * 1024;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_READS_DEFAULT: usize = 32;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_READS_MIN: usize = 4;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_COOP_YIELD_READS_MAX: usize = 128;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT: usize = 2;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_MIN: usize = 1;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_MAX: usize = 32;
#[cfg(feature = "asset-upload-http")]
const HTTP_INGRESS_ADAPTIVE_FAIRNESS_DEFAULT: usize = 0;
#[cfg(feature = "asset-upload-http")]
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
const fn configured_http_rx_buf_target() -> usize {
    let configured = match option_env!("MEDITAMER_HTTP_RX_BUF_TARGET") {
        Some(v) => Some(v),
        None => option_env!("HTTP_RX_BUF_TARGET"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(bytes) if bytes >= HTTP_RX_BUF_TARGET_MIN && bytes <= HTTP_RX_BUF_TARGET_MAX => bytes,
        _ => HTTP_RX_BUF_TARGET_DEFAULT,
    }
}
#[cfg(feature = "asset-upload-http")]
const fn configured_http_ingress_coop_yield_bytes() -> usize {
    let configured = match option_env!("MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES") {
        Some(v) => Some(v),
        None => option_env!("HTTP_INGRESS_COOP_YIELD_BYTES"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(bytes)
            if bytes >= HTTP_INGRESS_COOP_YIELD_BYTES_MIN
                && bytes <= HTTP_INGRESS_COOP_YIELD_BYTES_MAX =>
        {
            bytes
        }
        _ => HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT,
    }
}
#[cfg(feature = "asset-upload-http")]
const fn configured_http_ingress_coop_yield_reads() -> u32 {
    let configured = match option_env!("MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS") {
        Some(v) => Some(v),
        None => option_env!("HTTP_INGRESS_COOP_YIELD_READS"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(reads)
            if reads >= HTTP_INGRESS_COOP_YIELD_READS_MIN
                && reads <= HTTP_INGRESS_COOP_YIELD_READS_MAX =>
        {
            reads as u32
        }
        _ => HTTP_INGRESS_COOP_YIELD_READS_DEFAULT as u32,
    }
}
#[cfg(feature = "asset-upload-http")]
const fn configured_http_ingress_try_drain_interval_reads() -> u32 {
    let configured = match option_env!("MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS") {
        Some(v) => Some(v),
        None => option_env!("HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(reads)
            if reads >= HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_MIN
                && reads <= HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_MAX =>
        {
            reads as u32
        }
        _ => HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT as u32,
    }
}
#[cfg(feature = "asset-upload-http")]
const fn configured_http_ingress_adaptive_fairness() -> bool {
    let configured = match option_env!("MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS") {
        Some(v) => Some(v),
        None => option_env!("HTTP_INGRESS_ADAPTIVE_FAIRNESS"),
    };
    let parsed = match configured {
        Some(v) => parse_ascii_usize(v),
        None => None,
    };
    match parsed {
        Some(1) => true,
        Some(0) => false,
        _ => HTTP_INGRESS_ADAPTIVE_FAIRNESS_DEFAULT != 0,
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
#[cfg(all(feature = "asset-upload-http", feature = "psram-alloc"))]
// Override at build time via MEDITAMER_HTTP_RX_BUF_TARGET
// (fallback HTTP_RX_BUF_TARGET).
pub(crate) const HTTP_RX_BUF_TARGET_BYTES: usize = configured_http_rx_buf_target();
#[cfg(all(feature = "asset-upload-http", not(feature = "psram-alloc")))]
pub(crate) const HTTP_RX_BUF_TARGET_BYTES: usize = 2048;
#[cfg(feature = "asset-upload-http")]
// Override at build time via MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES
// (fallback HTTP_INGRESS_COOP_YIELD_BYTES).
pub(crate) const HTTP_INGRESS_COOP_YIELD_BYTES: usize = configured_http_ingress_coop_yield_bytes();
#[cfg(feature = "asset-upload-http")]
// Override at build time via MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS
// (fallback HTTP_INGRESS_COOP_YIELD_READS).
pub(crate) const HTTP_INGRESS_COOP_YIELD_READS: u32 = configured_http_ingress_coop_yield_reads();
#[cfg(feature = "asset-upload-http")]
// Override at build time via MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS
// (fallback HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS).
pub(crate) const HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS: u32 =
    configured_http_ingress_try_drain_interval_reads();
#[cfg(feature = "asset-upload-http")]
// Override at build time via MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS
// (fallback HTTP_INGRESS_ADAPTIVE_FAIRNESS): 0=off, 1=on.
pub(crate) const HTTP_INGRESS_ADAPTIVE_FAIRNESS: bool = configured_http_ingress_adaptive_fairness();
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
