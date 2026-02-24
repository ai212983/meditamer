use embassy_time::Instant;
use esp_hal::{gpio::Output, uart::Uart, Async};
use meditamer::{
    inkplate_hal::InkplateHal,
    platform::{BusyDelay, HalI2c},
};
use sdcard::probe;

use crate::app::store::ModeStore;

pub(crate) type InkplateDriver = InkplateHal<HalI2c<'static>, BusyDelay>;
pub(crate) type SerialUart = Uart<'static, Async>;
pub(crate) type SdProbeDriver = probe::SdCardProbe<'static>;
pub(crate) use sdcard::{SD_PATH_MAX, SD_WRITE_MAX};
pub(crate) const SD_UPLOAD_CHUNK_MAX: usize = 1024;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_SSID_MAX: usize = 32;
#[cfg(feature = "asset-upload-http")]
pub(crate) const WIFI_PASSWORD_MAX: usize = 64;

#[derive(Clone, Copy)]
pub(crate) enum AppEvent {
    Refresh { uptime_seconds: u32 },
    BatteryTick,
    TimeSync(TimeSyncCommand),
    TouchIrq,
    StartTouchCalibrationWizard,
    ForceRepaint,
    ForceMarbleRepaint,
}

#[derive(Clone, Copy)]
pub(crate) enum StorageCommand {
    Probe,
    RwVerify {
        lba: u32,
    },
    FatList {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRead {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatWrite {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatStat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatMkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRemove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRename {
        src_path: [u8; SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; SD_PATH_MAX],
        dst_path_len: u8,
    },
    FatAppend {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatTruncate {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
}

pub(crate) type SdCommand = StorageCommand;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdCommandKind {
    Probe,
    RwVerify,
    FatList,
    FatRead,
    FatWrite,
    FatStat,
    FatMkdir,
    FatRemove,
    FatRename,
    FatAppend,
    FatTruncate,
}

#[derive(Clone, Copy)]
pub(crate) struct SdRequest {
    pub(crate) id: u32,
    pub(crate) command: SdCommand,
}

#[derive(Clone, Copy)]
pub(crate) struct SdResult {
    pub(crate) id: u32,
    pub(crate) kind: SdCommandKind,
    pub(crate) ok: bool,
    pub(crate) code: SdResultCode,
    pub(crate) attempts: u8,
    pub(crate) duration_ms: u32,
}

pub(crate) type SdResultCode = sdcard::runtime::SdRuntimeResultCode;

#[cfg_attr(not(feature = "asset-upload-http"), allow(dead_code))]
pub(crate) enum SdUploadCommand {
    Begin {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        expected_size: u32,
    },
    Chunk {
        data: [u8; SD_UPLOAD_CHUNK_MAX],
        data_len: u16,
    },
    Commit,
    Abort,
    Mkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    Remove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
}

pub(crate) struct SdUploadRequest {
    pub(crate) command: SdUploadCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdUploadResultCode {
    Ok,
    Busy,
    SessionNotActive,
    InvalidPath,
    NotFound,
    NotEmpty,
    SizeMismatch,
    PowerOnFailed,
    InitFailed,
    OperationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SdUploadResult {
    pub(crate) ok: bool,
    pub(crate) code: SdUploadResultCode,
    pub(crate) bytes_written: u32,
}

#[cfg(feature = "asset-upload-http")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WifiCredentials {
    pub(crate) ssid: [u8; WIFI_SSID_MAX],
    pub(crate) ssid_len: u8,
    pub(crate) password: [u8; WIFI_PASSWORD_MAX],
    pub(crate) password_len: u8,
}

#[derive(Clone, Copy)]
pub(crate) enum SdPowerRequest {
    On,
    Off,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncCommand {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncState {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
    pub(crate) sync_instant: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    Clock,
    Suminagashi,
    Shanshui,
}

impl DisplayMode {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Clock => Self::Suminagashi,
            Self::Suminagashi => Self::Shanshui,
            Self::Shanshui => Self::Clock,
        }
    }

    pub(crate) fn as_persisted(self) -> u8 {
        match self {
            Self::Clock => 0,
            Self::Suminagashi => 1,
            Self::Shanshui => 2,
        }
    }

    pub(crate) fn from_persisted(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clock),
            1 => Some(Self::Suminagashi),
            2 => Some(Self::Shanshui),
            _ => None,
        }
    }

    pub(crate) fn toggled_reverse(self) -> Self {
        match self {
            Self::Clock => Self::Shanshui,
            Self::Suminagashi => Self::Clock,
            Self::Shanshui => Self::Suminagashi,
        }
    }
}

#[allow(unused_imports)]
pub(crate) use super::touch::types::{
    TouchEvent, TouchEventKind, TouchIrqPin, TouchPipelineInput, TouchSampleFrame,
    TouchSwipeDirection, TouchTraceSample, TouchWizardSessionEvent, TouchWizardSwipeTraceSample,
};

#[derive(Clone, Copy)]
pub(crate) struct TapTraceSample {
    pub(crate) t_ms: u64,
    pub(crate) tap_src: u8,
    pub(crate) seq_count: u8,
    pub(crate) tap_candidate: u8,
    pub(crate) cand_src: u8,
    pub(crate) state_id: u8,
    pub(crate) reject_reason: u8,
    pub(crate) candidate_score: u16,
    pub(crate) window_ms: u16,
    pub(crate) cooldown_active: u8,
    pub(crate) jerk_l1: i32,
    pub(crate) motion_veto: u8,
    pub(crate) gyro_l1: i32,
    pub(crate) int1: u8,
    pub(crate) int2: u8,
    pub(crate) power_good: i16,
    pub(crate) battery_percent: i16,
    pub(crate) gx: i16,
    pub(crate) gy: i16,
    pub(crate) gz: i16,
    pub(crate) ax: i16,
    pub(crate) ay: i16,
    pub(crate) az: i16,
}

pub(crate) struct DisplayContext {
    pub(crate) inkplate: InkplateDriver,
    pub(crate) mode_store: ModeStore<'static>,
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
