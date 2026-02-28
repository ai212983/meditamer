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

pub(super) fn app_state_command_for_serial(cmd: SerialCommand) -> Option<AppStateCommand> {
    match cmd {
        SerialCommand::StateSet { operation } => Some(operation.as_state_command()),
        SerialCommand::StateDiag { kind, targets } => {
            Some(AppStateCommand::SetDiag { kind, targets })
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStart => Some(AppStateCommand::SetUpload(true)),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStop => Some(AppStateCommand::SetUpload(false)),
        _ => None,
    }
}

pub(super) fn serial_command_event_and_responses(
    cmd: SerialCommand,
) -> (
    Option<AppEvent>,
    Option<SdCommand>,
    &'static [u8],
    &'static [u8],
) {
    match cmd {
        SerialCommand::TimeSync(cmd) => (
            Some(AppEvent::TimeSync(cmd)),
            None,
            b"TIMESET OK\r\n",
            b"TIMESET BUSY\r\n",
        ),
        SerialCommand::TouchWizard => (
            Some(AppEvent::StartTouchCalibrationWizard),
            None,
            b"TOUCH_WIZARD OK\r\n",
            b"TOUCH_WIZARD BUSY\r\n",
        ),
        SerialCommand::Repaint => (
            Some(AppEvent::ForceRepaint),
            None,
            b"REPAINT OK\r\n",
            b"REPAINT BUSY\r\n",
        ),
        SerialCommand::RepaintMarble => (
            Some(AppEvent::ForceMarbleRepaint),
            None,
            b"REPAINT_MARBLE OK\r\n",
            b"REPAINT_MARBLE BUSY\r\n",
        ),
        SerialCommand::Probe => (
            None,
            Some(SdCommand::Probe),
            b"SDPROBE OK\r\n",
            b"SDPROBE BUSY\r\n",
        ),
        SerialCommand::RwVerify { lba } => (
            None,
            Some(SdCommand::RwVerify { lba }),
            b"SDRWVERIFY OK\r\n",
            b"SDRWVERIFY BUSY\r\n",
        ),
        SerialCommand::FatList { path, path_len } => (
            None,
            Some(SdCommand::FatList { path, path_len }),
            b"SDFATLS OK\r\n",
            b"SDFATLS BUSY\r\n",
        ),
        SerialCommand::FatRead { path, path_len } => (
            None,
            Some(SdCommand::FatRead { path, path_len }),
            b"SDFATREAD OK\r\n",
            b"SDFATREAD BUSY\r\n",
        ),
        SerialCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        } => (
            None,
            Some(SdCommand::FatWrite {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATWRITE OK\r\n",
            b"SDFATWRITE BUSY\r\n",
        ),
        SerialCommand::FatStat { path, path_len } => (
            None,
            Some(SdCommand::FatStat { path, path_len }),
            b"SDFATSTAT OK\r\n",
            b"SDFATSTAT BUSY\r\n",
        ),
        SerialCommand::FatMkdir { path, path_len } => (
            None,
            Some(SdCommand::FatMkdir { path, path_len }),
            b"SDFATMKDIR OK\r\n",
            b"SDFATMKDIR BUSY\r\n",
        ),
        SerialCommand::FatRemove { path, path_len } => (
            None,
            Some(SdCommand::FatRemove { path, path_len }),
            b"SDFATRM OK\r\n",
            b"SDFATRM BUSY\r\n",
        ),
        SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => (
            None,
            Some(SdCommand::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            }),
            b"SDFATREN OK\r\n",
            b"SDFATREN BUSY\r\n",
        ),
        SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        } => (
            None,
            Some(SdCommand::FatAppend {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATAPPEND OK\r\n",
            b"SDFATAPPEND BUSY\r\n",
        ),
        SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        } => (
            None,
            Some(SdCommand::FatTruncate {
                path,
                path_len,
                size,
            }),
            b"SDFATTRUNC OK\r\n",
            b"SDFATTRUNC BUSY\r\n",
        ),
        SerialCommand::TouchWizardDump => {
            unreachable!("touch wizard dump command is handled inline")
        }
        SerialCommand::Ping => unreachable!("ping command is handled inline"),
        SerialCommand::Metrics
        | SerialCommand::MetricsNet
        | SerialCommand::TelemetryStatus
        | SerialCommand::TelemetrySet { .. } => {
            unreachable!("metrics command is handled inline")
        }
        SerialCommand::AllocatorStatus => unreachable!("allocator command is handled inline"),
        SerialCommand::AllocatorAllocProbe { .. } => {
            unreachable!("allocator allocation probe command is handled inline")
        }
        SerialCommand::SdWait { .. } => unreachable!("sdwait command is handled inline"),
        SerialCommand::DiagGet => unreachable!("diag get command is handled inline"),
        SerialCommand::StateGet => unreachable!("state get command is handled inline"),
        SerialCommand::StateSet { .. } => unreachable!("state set command is handled inline"),
        SerialCommand::StateDiag { .. } => unreachable!("state diag command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgSet { .. } => unreachable!("netcfg command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetCfgGet => unreachable!("netcfg command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStart => unreachable!("net command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStop => unreachable!("net command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStatus => unreachable!("net command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetRecover => unreachable!("net command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetListenerSet { .. } => unreachable!("net command is handled inline"),
    }
}
