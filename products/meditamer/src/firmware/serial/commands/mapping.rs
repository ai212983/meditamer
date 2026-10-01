use super::types::SerialCommand;
use crate::firmware::types::{AppEvent, SdCommand};

#[path = "storage_mapping.rs"]
mod storage_mapping;

pub(in crate::firmware::serial) fn serial_command_event_and_responses(
    cmd: SerialCommand,
) -> (
    Option<AppEvent>,
    Option<SdCommand>,
    &'static [u8],
    &'static [u8],
) {
    if let Some(mapping) = storage_mapping::map_repaint_command(cmd) {
        return mapping;
    }
    if let Some(mapping) = storage_mapping::map_probe_command(cmd) {
        return mapping;
    }
    if let Some(mapping) = storage_mapping::map_fat_read_command(cmd) {
        return mapping;
    }
    if let Some(mapping) = storage_mapping::map_fat_write_command(cmd) {
        return mapping;
    }
    if let Some(mapping) = storage_mapping::map_fat_dir_command(cmd) {
        return mapping;
    }
    match cmd {
        SerialCommand::DisplayPause { .. } => unreachable!("display pause is handled inline"),
        SerialCommand::Repaint { .. }
        | SerialCommand::Probe
        | SerialCommand::RwVerify { .. }
        | SerialCommand::FatList { .. }
        | SerialCommand::FatRead { .. }
        | SerialCommand::FatWrite { .. }
        | SerialCommand::FatStat { .. }
        | SerialCommand::FatMkdir { .. }
        | SerialCommand::FatRemove { .. }
        | SerialCommand::FatRename { .. }
        | SerialCommand::FatAppend { .. }
        | SerialCommand::FatTruncate { .. } => {
            unreachable!("queued command is mapped above")
        }
        SerialCommand::Ping
        | SerialCommand::FirmwareFactoryBoot
        | SerialCommand::ObservationFixture(_)
        | SerialCommand::ObservationPanelCycle(_) => {
            unreachable!("local command is handled inline")
        }
        SerialCommand::UiCycleStep { .. } => unreachable!("UI step command is handled inline"),
        #[cfg(feature = "ui-provider-fixture")]
        SerialCommand::UiProviderFixtureStep => {
            unreachable!("UI provider fixture command is handled inline")
        }
        SerialCommand::Metrics
        | SerialCommand::AcquisitionWindow
        | SerialCommand::TouchSchedReset
        | SerialCommand::Scheduler { .. }
        | SerialCommand::MetricsNet
        | SerialCommand::TelemetryStatus
        | SerialCommand::TelemetrySet { .. }
        | SerialCommand::StackStatus
        | SerialCommand::ConsoleStats
        | SerialCommand::MountainStatus
        | SerialCommand::AllocatorStatus => {
            unreachable!("diagnostic command is handled inline")
        }
        #[cfg(feature = "firmware-trace")]
        SerialCommand::Trace { .. } => unreachable!("trace command is handled inline"),
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
        SerialCommand::TimeReply { .. } | SerialCommand::TimeSync | SerialCommand::TimeGet => {
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
