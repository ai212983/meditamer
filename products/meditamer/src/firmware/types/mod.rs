mod app_event;
mod base;
pub mod bus_owner;
pub(crate) mod i2c;
pub mod imu_bus_owner;
mod modes;
pub(crate) mod rtc_diagnostics;
mod sd;
mod serial;
mod trace;
mod ui_cycle;
mod wall_clock;

pub use app_event::{AppEvent, UiCycleTarget};
pub use base::{
    DisplayContext, ImuSampleTimer, InkplateDriver, InkplateEnvironmentDriver, InkplateImuDriver,
    InkplateRtcDriver, InkplateTouchDriver, PanelI2cDevice, PanelPinHold, RtcI2cDevice,
    SdProbeDriver, SerialUart, SerialWriter, SharedI2cBus, SharedI2cDevice, TouchRetryTimer,
    SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX, SD_WRITE_MAX,
};
#[cfg(feature = "asset-upload-http")]
pub(crate) use base::{
    HTTP_INGRESS_ADAPTIVE_FAIRNESS, HTTP_INGRESS_COOP_YIELD_BYTES, HTTP_INGRESS_COOP_YIELD_READS,
    HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS, HTTP_RX_BUF_TARGET_BYTES,
};
pub(crate) use i2c::timing_snapshot as i2c_timing_snapshot;
pub(crate) use i2c::wait_snapshot as i2c_wait_snapshot;
pub use i2c::{
    configured_i2c_device, i2c_config, imu_i2c_device, panel_i2c_device, shared_i2c_bus,
    shared_i2c_device, touch_i2c_device, ConfiguredI2cDevice,
};
pub(crate) use modes::AppStateApplyAck;
pub(crate) use sd::{
    SdCommand, SdCommandKind, SdPowerRequest, SdRequest, SdResult, SdResultCode, SdUploadCommand,
    SdUploadRequest, SdUploadResult, SdUploadResultCode,
};
pub(crate) use serial::SerialStatusEvent;
pub(crate) use trace::TapTraceSample;
pub(crate) use ui_cycle::{UiCycleStepAck, UiCycleStepStatus};
pub(crate) use wall_clock::WallClockQueryResult;
// Wi-Fi configuration types (`NetConfigSet`, `WifiCredentials`, ...) moved to
// `netstack::config` in the product/target axis completion plan's Phase 4;
// consumers reach them there directly rather than through this facade.

#[allow(unused_imports)]
pub(crate) use super::touch::types::{
    Gpio36InputPin, TouchEvent, TouchEventKind, TouchPipelineInput, TouchSampleFrame, TouchStatus,
    TouchSwipeDirection,
};
