use super::types::SerialCommand;
use crate::firmware::types::{AppEvent, SdCommand};

pub(in crate::firmware::serial) fn serial_command_event_and_responses(
    cmd: SerialCommand,
) -> (
    Option<AppEvent>,
    Option<SdCommand>,
    &'static [u8],
    &'static [u8],
) {
    match cmd {
        SerialCommand::Repaint => (
            Some(AppEvent::ForceRepaint),
            None,
            b"REPAINT OK\r\n",
            b"REPAINT BUSY\r\n",
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
        SerialCommand::Ping | SerialCommand::FirmwareFactoryBoot => {
            unreachable!("local command is handled inline")
        }
        SerialCommand::UiCycleStep => unreachable!("UI step command is handled inline"),
        #[cfg(feature = "ui-provider-fixture")]
        SerialCommand::UiProviderFixtureStep => {
            unreachable!("UI provider fixture command is handled inline")
        }
        SerialCommand::Metrics
        | SerialCommand::TouchSchedReset
        | SerialCommand::Scheduler { .. }
        | SerialCommand::MetricsNet
        | SerialCommand::TelemetryStatus
        | SerialCommand::TelemetrySet { .. }
        | SerialCommand::StackStatus
        | SerialCommand::AllocatorStatus => {
            unreachable!("diagnostic command is handled inline")
        }
        SerialCommand::AllocatorAllocProbe { .. } => {
            unreachable!("allocator allocation probe command is handled inline")
        }
        #[cfg(feature = "ble-foundation")]
        SerialCommand::BleProbeStart
        | SerialCommand::BleProbeStatus
        | SerialCommand::BlePhase1sStart { .. }
        | SerialCommand::BlePhase1sStatus
        | SerialCommand::RadioHandoffAcquire { .. }
        | SerialCommand::RadioHandoffRelease { .. }
        | SerialCommand::RadioHandoffStatus => {
            unreachable!("BLE probe command is handled inline")
        }
        SerialCommand::SdWait { .. } => unreachable!("sdwait command is handled inline"),
        SerialCommand::DiagGet => unreachable!("diag get command is handled inline"),
        SerialCommand::TimeSet { .. } | SerialCommand::TimeGet => {
            unreachable!("time command is handled inline")
        }
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
