use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use esp_hal::{gpio::Output, timer::OneShotTimer, uart::Uart, Async};
use inkplate_tempera::{
    adapters::BusyDelay, environment::InkplateEnvironment, touch::InkplateTouch, InkplateHal,
};
use sdcard::probe;

use super::super::app_state::AppStateStore;

pub type SharedI2cDevice = super::i2c::ConfiguredI2cDevice<100>;
/// A panel-owned shared-bus handle. Its I2C configuration is applied while
/// holding the bus mutex before each PMIC, expander, or digital-pot transfer.
pub type PanelI2cDevice = super::bus_owner::RemoteI2cDevice;
pub type RtcI2cDevice = super::bus_owner::RemoteI2cDevice;
/// The shared bus itself, rather than one caller's device handle on it --
/// callers that need to construct fresh configured handles after a failed
/// initialization (typed observation subscriptions plan Phase 4) take this
/// instead.
pub type SharedI2cBus = Mutex<CriticalSectionRawMutex, super::i2c::SharedI2cTransport>;
pub type InkplateDriver = InkplateHal<PanelI2cDevice, BusyDelay>;
pub type InkplateEnvironmentDriver = InkplateEnvironment<SharedI2cDevice>;
pub type InkplateImuDriver = super::imu_bus_owner::ImuClient;
pub type InkplateTouchDriver = InkplateTouch<super::i2c::ConfiguredI2cDevice<100, 2>>;
pub type TouchRetryTimer = OneShotTimer<'static, Async>;
pub type ImuSampleTimer = OneShotTimer<'static, Async>;
pub type InkplateRtcDriver =
    rtc::driver::Pcf85063a<RtcI2cDevice, super::rtc_diagnostics::InkplateRtcDiagnostics>;
pub type SerialUart = Uart<'static, Async>;
/// Output capability backed by the console's shared, contiguous-record arbiter.
/// Only the serial RX task retains the physical UART handle.
pub struct SerialWriter;
pub type SdProbeDriver = probe::SdCardProbe<'static>;
pub use sdcard::{SD_PATH_MAX, SD_WRITE_MAX};
#[cfg(feature = "asset-upload-http")]
const SD_UPLOAD_CHUNK_MAX_DEFAULT: usize = 65_536;
#[cfg(feature = "asset-upload-http")]
const SD_UPLOAD_CHUNK_MAX_MIN: usize = 4_096;
#[cfg(feature = "asset-upload-http")]
const SD_UPLOAD_CHUNK_MAX_MAX: usize = 65_536;
#[cfg(feature = "asset-upload-http")]
const HTTP_RX_BUF_TARGET_DEFAULT: usize = 65_536;
#[cfg(feature = "asset-upload-http")]
const HTTP_RX_BUF_TARGET_MIN: usize = 8_192;
#[cfg(feature = "asset-upload-http")]
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
#[cfg(feature = "asset-upload-http")]
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
#[cfg(feature = "asset-upload-http")]
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
#[cfg(feature = "asset-upload-http")]
// Larger upload chunks reduce per-chunk SD roundtrip overhead and improve
// sustained HTTP upload throughput when PSRAM is available.
// Override at build time via MEDITAMER_SD_UPLOAD_CHUNK_MAX (fallback SD_UPLOAD_CHUNK_MAX).
pub const SD_UPLOAD_CHUNK_MAX: usize = configured_sd_upload_chunk_max();
#[cfg(not(feature = "asset-upload-http"))]
pub const SD_UPLOAD_CHUNK_MAX: usize = 1024;
#[cfg(feature = "asset-upload-http")]
// Override at build time via MEDITAMER_HTTP_RX_BUF_TARGET
// (fallback HTTP_RX_BUF_TARGET).
pub(crate) const HTTP_RX_BUF_TARGET_BYTES: usize = configured_http_rx_buf_target();
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

pub struct DisplayContext {
    pub inkplate: InkplateDriver,
    pub app_state_store: AppStateStore,
    pub _panel_pins: PanelPinHold<'static>,
    /// Panel-native Gray4 framebuffer, when PSRAM had room for it. Present
    /// so the panel's eight physical grey levels can be driven directly --
    /// the LVGL path thresholds to black and white in `panel_blit::blit_l8`,
    /// so grayscale cannot reach the glass through it. `None` simply means
    /// grayscale refreshes are unavailable; nothing else degrades.
    pub gray4_framebuffer: Option<&'static mut [u8]>,
}

pub struct PanelPinHold<'d> {
    pub _cl: Output<'d>,
    pub _le: Output<'d>,
    pub _d0: Output<'d>,
    pub _d1: Output<'d>,
    pub _d2: Output<'d>,
    pub _d3: Output<'d>,
    pub _d4: Output<'d>,
    pub _d5: Output<'d>,
    pub _d6: Output<'d>,
    pub _d7: Output<'d>,
    pub _ckv: Output<'d>,
    pub _sph: Output<'d>,
}
