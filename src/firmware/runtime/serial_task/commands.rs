#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::WifiCredentials;
use crate::firmware::types::{
    AppEvent, RuntimeMode, RuntimeServicesUpdate, SdCommand, TimeSyncCommand, SD_PATH_MAX,
    SD_WRITE_MAX,
};

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
    ModeStatus,
    ModeSet {
        operation: ModeSetOperation,
    },
    RunMode {
        mode: RuntimeMode,
    },
    #[cfg(feature = "asset-upload-http")]
    WifiSet {
        credentials: WifiCredentials,
    },
}

#[derive(Clone, Copy)]
pub(super) enum ModeSetOperation {
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

impl ModeSetOperation {
    pub(super) fn as_update(self) -> RuntimeServicesUpdate {
        match self {
            Self::Upload(enabled) => RuntimeServicesUpdate::Upload(enabled),
            Self::AssetReads(enabled) => RuntimeServicesUpdate::AssetReads(enabled),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SdWaitTarget {
    Next,
    Last,
    Id(u32),
}

pub(super) fn runtime_services_update_for_command(
    cmd: SerialCommand,
) -> Option<RuntimeServicesUpdate> {
    match cmd {
        SerialCommand::ModeSet { operation } => Some(operation.as_update()),
        SerialCommand::RunMode { mode } => Some(RuntimeServicesUpdate::Replace(mode.as_services())),
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
        SerialCommand::ModeStatus => unreachable!("mode status command is handled inline"),
        SerialCommand::ModeSet { .. } => unreachable!("mode set command is handled inline"),
        SerialCommand::RunMode { .. } => unreachable!("runmode command is handled inline"),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::WifiSet { .. } => unreachable!("wifiset command is handled inline"),
    }
}
