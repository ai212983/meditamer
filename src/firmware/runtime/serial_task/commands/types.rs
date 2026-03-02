use crate::firmware::app_state::{
    AppStateCommand, BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode,
};
#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::NetConfigSet;
use crate::firmware::types::{AppEvent, SdCommand, TimeSyncCommand, SD_PATH_MAX, SD_WRITE_MAX};

#[derive(Clone, Copy)]
pub(super) enum SerialCommand {
    Ping,
    TimeSync(TimeSyncCommand),
    TouchWizard,
    TouchWizardDump,
    Repaint,
    RepaintMarble,
    Metrics,
    MetricsNet,
    TelemetryStatus,
    TelemetrySet {
        operation: TelemetrySetOperation,
    },
    AllocatorStatus,
    AllocatorAllocProbe {
        bytes: u32,
    },
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
    SdWait {
        target: SdWaitTarget,
        timeout_ms: u32,
    },
    DiagGet,
    StateGet,
    StateSet {
        operation: StateSetOperation,
    },
    StateDiag {
        kind: DiagKind,
        targets: DiagTargets,
    },
    #[cfg(feature = "asset-upload-http")]
    NetCfgSet {
        config: NetConfigSet,
    },
    #[cfg(feature = "asset-upload-http")]
    NetCfgGet,
    #[cfg(feature = "asset-upload-http")]
    NetStart,
    #[cfg(feature = "asset-upload-http")]
    NetStop,
    #[cfg(feature = "asset-upload-http")]
    NetStatus,
    #[cfg(feature = "asset-upload-http")]
    NetRecover,
    #[cfg(feature = "asset-upload-http")]
    NetListenerSet {
        enabled: bool,
    },
}

#[derive(Clone, Copy)]
pub(super) enum StateSetOperation {
    Base(BaseMode),
    DayBackground(DayBackground),
    Overlay(OverlayMode),
    Upload(bool),
    AssetReads(bool),
}

#[derive(Clone, Copy)]
pub(super) enum TelemetryDomain {
    Wifi,
    Reassoc,
    Net,
    Http,
    Sd,
}

#[derive(Clone, Copy)]
pub(super) enum TelemetrySetOperation {
    Domain {
        domain: TelemetryDomain,
        enabled: bool,
    },
    All {
        enabled: bool,
    },
    Default,
}

impl StateSetOperation {
    pub(super) fn as_state_command(self) -> AppStateCommand {
        match self {
            Self::Base(mode) => AppStateCommand::SetBase(mode),
            Self::DayBackground(day_bg) => AppStateCommand::SetDayBackground(day_bg),
            Self::Overlay(overlay) => AppStateCommand::SetOverlay(overlay),
            Self::Upload(enabled) => AppStateCommand::SetUpload(enabled),
            Self::AssetReads(enabled) => AppStateCommand::SetAssets(enabled),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SdWaitTarget {
    Next,
    Last,
    Id(u32),
}
