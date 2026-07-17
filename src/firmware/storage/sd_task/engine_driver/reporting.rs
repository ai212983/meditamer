use core::fmt::Write;

use sdcard::fat::{FatEngineError, FatResult};

use super::super::super::super::types::{SdCommand, SdResultCode};
use super::super::serial_log::{self, SdSerialLine};

macro_rules! queue_sd_line {
    ($($arg:tt)*) => {{
        let mut line = SdSerialLine::new();
        let _ = write!(&mut line, $($arg)*);
        let _ = line.push_str("\r\n");
        let _ = serial_log::send(line);
    }};
}

pub(super) fn publish_result(
    command: SdCommand,
    result: FatResult,
    output: &[u8],
) -> (SdResultCode, bool) {
    match result {
        FatResult::Done => {
            publish_done(command);
            (SdResultCode::Ok, false)
        }
        FatResult::Listed { count } => {
            queue_sd_line!("sdfat[request]: ls_ok count={}", count);
            (SdResultCode::Ok, false)
        }
        FatResult::Read { bytes } => {
            let preview = bytes.min(64) as usize;
            let mut line = SdSerialLine::new();
            let _ = write!(
                &mut line,
                "sdfat[request]: read_ok bytes={} preview_hex=",
                bytes
            );
            for byte in &output[..preview] {
                let _ = write!(&mut line, "{:02x}", byte);
            }
            let _ = line.push_str("\r\n");
            let _ = serial_log::send(line);
            (SdResultCode::Ok, false)
        }
        FatResult::Stat(entry) => {
            let name =
                core::str::from_utf8(&entry.name[..entry.name_len as usize]).unwrap_or("<invalid>");
            let path = match &command {
                SdCommand::FatStat { path, path_len } => decode_path(path, *path_len),
                _ => "<invalid>",
            };
            queue_sd_line!(
                "sdfat[request]: stat_ok path={} kind={} name={} size={}",
                path,
                if entry.is_dir { "dir" } else { "file" },
                name,
                entry.size
            );
            (SdResultCode::Ok, false)
        }
        FatResult::Error(FatEngineError::Fat(sdcard::fat::SdFatError::InvalidPath)) => {
            publish_fat_error(command, &sdcard::fat::SdFatError::InvalidPath);
            (SdResultCode::InvalidPath, false)
        }
        FatResult::Error(FatEngineError::Fat(sdcard::fat::SdFatError::NotFound)) => {
            publish_fat_error(command, &sdcard::fat::SdFatError::NotFound);
            (SdResultCode::NotFound, false)
        }
        FatResult::Error(FatEngineError::Fat(err)) => {
            publish_fat_error(command, &err);
            (SdResultCode::OperationFailed, false)
        }
        FatResult::Error(err) => {
            let retryable = err.is_transport_failure();
            queue_sd_line!("sdfat[request]: engine_error={:?}", err);
            (SdResultCode::OperationFailed, retryable)
        }
    }
}

fn publish_fat_error(command: SdCommand, err: &sdcard::fat::SdFatError) {
    match command {
        SdCommand::FatList { path, path_len } => queue_sd_line!(
            "sdfat[request]: ls_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatRead { path, path_len } => queue_sd_line!(
            "sdfat[request]: read_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatWrite { path, path_len, .. } => queue_sd_line!(
            "sdfat[request]: write_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatStat { path, path_len } => queue_sd_line!(
            "sdfat[request]: stat_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatMkdir { path, path_len } => queue_sd_line!(
            "sdfat[request]: mkdir_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatRemove { path, path_len } => queue_sd_line!(
            "sdfat[request]: rm_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => queue_sd_line!(
            "sdfat[request]: ren_error src={} dst={} err={:?}",
            decode_path(&src_path, src_path_len),
            decode_path(&dst_path, dst_path_len),
            err
        ),
        SdCommand::FatAppend { path, path_len, .. } => queue_sd_line!(
            "sdfat[request]: append_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::FatTruncate { path, path_len, .. } => queue_sd_line!(
            "sdfat[request]: trunc_error path={} err={:?}",
            decode_path(&path, path_len),
            err
        ),
        SdCommand::Probe | SdCommand::RwVerify { .. } => {}
    }
}

fn publish_done(command: SdCommand) {
    match command {
        SdCommand::FatWrite {
            path,
            path_len,
            data_len,
            ..
        } => queue_sd_line!(
            "sdfat[request]: write_ok path={} bytes={} verify=ok",
            decode_path(&path, path_len),
            data_len
        ),
        SdCommand::FatMkdir { path, path_len } => queue_sd_line!(
            "sdfat[request]: mkdir_ok path={}",
            decode_path(&path, path_len)
        ),
        SdCommand::FatRemove { path, path_len } => queue_sd_line!(
            "sdfat[request]: rm_ok path={}",
            decode_path(&path, path_len)
        ),
        SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => queue_sd_line!(
            "sdfat[request]: ren_ok src={} dst={}",
            decode_path(&src_path, src_path_len),
            decode_path(&dst_path, dst_path_len)
        ),
        SdCommand::FatAppend {
            path,
            path_len,
            data_len,
            ..
        } => queue_sd_line!(
            "sdfat[request]: append_ok path={} bytes={}",
            decode_path(&path, path_len),
            data_len
        ),
        SdCommand::FatTruncate {
            path,
            path_len,
            size,
        } => queue_sd_line!(
            "sdfat[request]: trunc_ok path={} size={}",
            decode_path(&path, path_len),
            size
        ),
        _ => {}
    }
}

fn decode_path(path: &[u8], len: u8) -> &str {
    core::str::from_utf8(&path[..usize::from(len).min(path.len())]).unwrap_or("<invalid>")
}

pub(super) fn publish_list_entry(entry: &sdcard::fat::FatDirEntry) {
    let name = core::str::from_utf8(&entry.name[..entry.name_len as usize]).unwrap_or("<invalid>");
    queue_sd_line!(
        "sdfat[request]: ls {} name={} size={}",
        if entry.is_dir { "dir" } else { "file" },
        name,
        entry.size
    );
}
