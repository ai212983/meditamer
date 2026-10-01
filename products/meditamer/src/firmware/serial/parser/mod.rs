mod basic;
mod basic_groups;
mod firmware;
mod sd_control;
mod sdfat;
mod util;

use super::commands::SerialCommand;

pub(super) const SDWAIT_DEFAULT_TIMEOUT_MS: u32 = 10_000;

pub(super) fn parse_serial_command(line: &[u8]) -> Option<SerialCommand> {
    parse_basic_command(line)
        .or_else(|| parse_network_command(line))
        .or_else(|| parse_storage_command(line))
}

fn parse_basic_command(line: &[u8]) -> Option<SerialCommand> {
    basic_groups::parse_fixture_command(line)
        .or_else(|| basic_groups::parse_ble_command(line))
        .or_else(|| basic_groups::parse_repaint_and_trace_command(line))
        .or_else(|| basic_groups::parse_telemetry_command(line))
        .or_else(|| basic_groups::parse_state_time_command(line))
}

#[cfg(feature = "asset-upload-http")]
fn parse_network_command(line: &[u8]) -> Option<SerialCommand> {
    if let Some(config) = basic::parse_netcfg_set_command(line) {
        return Some(SerialCommand::NetCfgSet { config });
    }
    if basic::parse_netcfg_get_command(line) {
        return Some(SerialCommand::NetCfgGet);
    }
    if basic::parse_net_start_command(line) {
        return Some(SerialCommand::NetStart);
    }
    if basic::parse_net_stop_command(line) {
        return Some(SerialCommand::NetStop);
    }
    if basic::parse_net_status_command(line) {
        return Some(SerialCommand::NetStatus);
    }
    if basic::parse_net_recover_command(line) {
        return Some(SerialCommand::NetRecover);
    }
    if let Some(enabled) = basic::parse_net_listener_command(line) {
        return Some(SerialCommand::NetListenerSet { enabled });
    }
    None
}

#[cfg(not(feature = "asset-upload-http"))]
fn parse_network_command(_line: &[u8]) -> Option<SerialCommand> {
    None
}

fn parse_storage_command(line: &[u8]) -> Option<SerialCommand> {
    parse_allocator_probe_command(line)
        .or_else(|| parse_sd_control_command(line))
        .or_else(|| parse_sdfat_command(line))
}

fn parse_allocator_probe_command(line: &[u8]) -> Option<SerialCommand> {
    if basic::parse_allocator_status_command(line) {
        return Some(SerialCommand::AllocatorStatus);
    }
    if basic::parse_sdprobe_command(line) {
        return Some(SerialCommand::Probe);
    }
    None
}

fn parse_sd_control_command(line: &[u8]) -> Option<SerialCommand> {
    if let Some((target, timeout_ms)) = sd_control::parse_sdwait_command(line) {
        return Some(SerialCommand::SdWait { target, timeout_ms });
    }
    if let Some(lba) = sd_control::parse_sdrwverify_command(line) {
        return Some(SerialCommand::RwVerify { lba });
    }
    None
}

fn parse_sdfat_command(line: &[u8]) -> Option<SerialCommand> {
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
    None
}
