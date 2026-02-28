mod basic;
mod sd_control;
mod sdfat;
mod util;

use super::commands::SerialCommand;

pub(super) const SDWAIT_DEFAULT_TIMEOUT_MS: u32 = 10_000;

pub(super) fn parse_serial_command(line: &[u8]) -> Option<SerialCommand> {
    if basic::parse_repaint_marble_command(line) {
        return Some(SerialCommand::RepaintMarble);
    }
    if basic::parse_repaint_command(line) {
        return Some(SerialCommand::Repaint);
    }
    if basic::parse_touch_wizard_dump_command(line) {
        return Some(SerialCommand::TouchWizardDump);
    }
    if basic::parse_touch_wizard_command(line) {
        return Some(SerialCommand::TouchWizard);
    }
    if basic::parse_metrics_net_command(line) {
        return Some(SerialCommand::MetricsNet);
    }
    if basic::parse_telemetry_status_command(line) {
        return Some(SerialCommand::TelemetryStatus);
    }
    if let Some(operation) = basic::parse_telemetry_set_command(line) {
        return Some(SerialCommand::TelemetrySet { operation });
    }
    if basic::parse_metrics_command(line) {
        return Some(SerialCommand::Metrics);
    }
    if basic::parse_ping_command(line) {
        return Some(SerialCommand::Ping);
    }
    if let Some(bytes) = basic::parse_allocator_alloc_probe_command(line) {
        return Some(SerialCommand::AllocatorAllocProbe { bytes });
    }
    if basic::parse_diag_get_command(line) {
        return Some(SerialCommand::DiagGet);
    }
    if basic::parse_state_get_command(line) {
        return Some(SerialCommand::StateGet);
    }
    if let Some(operation) = basic::parse_state_set_command(line) {
        return Some(SerialCommand::StateSet { operation });
    }
    if let Some((kind, targets)) = basic::parse_state_diag_command(line) {
        return Some(SerialCommand::StateDiag { kind, targets });
    }
    #[cfg(feature = "asset-upload-http")]
    if let Some(config) = basic::parse_netcfg_set_command(line) {
        return Some(SerialCommand::NetCfgSet { config });
    }
    #[cfg(feature = "asset-upload-http")]
    if basic::parse_netcfg_get_command(line) {
        return Some(SerialCommand::NetCfgGet);
    }
    #[cfg(feature = "asset-upload-http")]
    if basic::parse_net_start_command(line) {
        return Some(SerialCommand::NetStart);
    }
    #[cfg(feature = "asset-upload-http")]
    if basic::parse_net_stop_command(line) {
        return Some(SerialCommand::NetStop);
    }
    #[cfg(feature = "asset-upload-http")]
    if basic::parse_net_status_command(line) {
        return Some(SerialCommand::NetStatus);
    }
    #[cfg(feature = "asset-upload-http")]
    if basic::parse_net_recover_command(line) {
        return Some(SerialCommand::NetRecover);
    }
    if basic::parse_allocator_status_command(line) {
        return Some(SerialCommand::AllocatorStatus);
    }
    if basic::parse_sdprobe_command(line) {
        return Some(SerialCommand::Probe);
    }
    if let Some((target, timeout_ms)) = sd_control::parse_sdwait_command(line) {
        return Some(SerialCommand::SdWait { target, timeout_ms });
    }
    if let Some(lba) = sd_control::parse_sdrwverify_command(line) {
        return Some(SerialCommand::RwVerify { lba });
    }
    if let Some((path, path_len)) = sdfat::parse_sdfatls_command(line) {
        return Some(SerialCommand::FatList { path, path_len });
    }
    if let Some((path, path_len)) = sdfat::parse_sdfatread_command(line) {
        return Some(SerialCommand::FatRead { path, path_len });
    }
    if let Some((path, path_len, data, data_len)) = sdfat::parse_sdfatwrite_command(line) {
        return Some(SerialCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len)) = sdfat::parse_sdfatstat_command(line) {
        return Some(SerialCommand::FatStat { path, path_len });
    }
    if let Some((path, path_len)) = sdfat::parse_sdfatmkdir_command(line) {
        return Some(SerialCommand::FatMkdir { path, path_len });
    }
    if let Some((path, path_len)) = sdfat::parse_sdfatrm_command(line) {
        return Some(SerialCommand::FatRemove { path, path_len });
    }
    if let Some((src_path, src_path_len, dst_path, dst_path_len)) =
        sdfat::parse_sdfatren_command(line)
    {
        return Some(SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        });
    }
    if let Some((path, path_len, data, data_len)) = sdfat::parse_sdfatappend_command(line) {
        return Some(SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len, size)) = sdfat::parse_sdfattrunc_command(line) {
        return Some(SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        });
    }

    basic::parse_timeset_command(line).map(SerialCommand::TimeSync)
}
