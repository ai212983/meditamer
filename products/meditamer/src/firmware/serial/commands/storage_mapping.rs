use super::super::SerialCommand;
use crate::firmware::types::{AppEvent, SdCommand};

pub(super) type QueuedMapping = (
    Option<AppEvent>,
    Option<SdCommand>,
    &'static [u8],
    &'static [u8],
);

pub(super) fn map_repaint_command(cmd: SerialCommand) -> Option<QueuedMapping> {
    match cmd {
        SerialCommand::Repaint { request_id } => Some((
            Some(AppEvent::ForceRepaint { request_id }),
            None,
            b"REPAINT OK\r\n",
            b"REPAINT BUSY\r\n",
        )),
        _ => None,
    }
}

pub(super) fn map_probe_command(cmd: SerialCommand) -> Option<QueuedMapping> {
    match cmd {
        SerialCommand::Probe => Some((
            None,
            Some(SdCommand::Probe),
            b"SDPROBE OK\r\n",
            b"SDPROBE BUSY\r\n",
        )),
        SerialCommand::RwVerify { lba } => Some((
            None,
            Some(SdCommand::RwVerify { lba }),
            b"SDRWVERIFY OK\r\n",
            b"SDRWVERIFY BUSY\r\n",
        )),
        _ => None,
    }
}

pub(super) fn map_fat_read_command(cmd: SerialCommand) -> Option<QueuedMapping> {
    match cmd {
        SerialCommand::FatList { path, path_len } => Some((
            None,
            Some(SdCommand::FatList { path, path_len }),
            b"SDFATLS OK\r\n",
            b"SDFATLS BUSY\r\n",
        )),
        SerialCommand::FatRead { path, path_len } => Some((
            None,
            Some(SdCommand::FatRead { path, path_len }),
            b"SDFATREAD OK\r\n",
            b"SDFATREAD BUSY\r\n",
        )),
        SerialCommand::FatStat { path, path_len } => Some((
            None,
            Some(SdCommand::FatStat { path, path_len }),
            b"SDFATSTAT OK\r\n",
            b"SDFATSTAT BUSY\r\n",
        )),
        _ => None,
    }
}

pub(super) fn map_fat_write_command(cmd: SerialCommand) -> Option<QueuedMapping> {
    match cmd {
        SerialCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        } => Some((
            None,
            Some(SdCommand::FatWrite {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATWRITE OK\r\n",
            b"SDFATWRITE BUSY\r\n",
        )),
        SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        } => Some((
            None,
            Some(SdCommand::FatAppend {
                path,
                path_len,
                data,
                data_len,
            }),
            b"SDFATAPPEND OK\r\n",
            b"SDFATAPPEND BUSY\r\n",
        )),
        SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        } => Some((
            None,
            Some(SdCommand::FatTruncate {
                path,
                path_len,
                size,
            }),
            b"SDFATTRUNC OK\r\n",
            b"SDFATTRUNC BUSY\r\n",
        )),
        _ => None,
    }
}

pub(super) fn map_fat_dir_command(cmd: SerialCommand) -> Option<QueuedMapping> {
    match cmd {
        SerialCommand::FatMkdir { path, path_len } => Some((
            None,
            Some(SdCommand::FatMkdir { path, path_len }),
            b"SDFATMKDIR OK\r\n",
            b"SDFATMKDIR BUSY\r\n",
        )),
        SerialCommand::FatRemove { path, path_len } => Some((
            None,
            Some(SdCommand::FatRemove { path, path_len }),
            b"SDFATRM OK\r\n",
            b"SDFATRM BUSY\r\n",
        )),
        SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => Some((
            None,
            Some(SdCommand::FatRename {
                src_path,
                src_path_len,
                dst_path,
                dst_path_len,
            }),
            b"SDFATREN OK\r\n",
            b"SDFATREN BUSY\r\n",
        )),
        _ => None,
    }
}
