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
pub(crate) const SD_UPLOAD_CHUNK_MAX: usize = 8192;
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
