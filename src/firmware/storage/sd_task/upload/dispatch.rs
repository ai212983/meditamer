use super::super::super::super::types::{SdProbeDriver, SdUploadRequest, SdUploadResult};
use super::path_ops::{handle_mkdir, handle_remove, handle_stat};
use super::stream::{handle_abort, handle_begin, handle_chunk, handle_commit, SdUploadBegin};
use super::types::{
    split_upload_command, SdUploadSession, UploadCommandGroup, UploadPathCommand,
    UploadStreamCommand,
};
use embassy_time::Instant;
use sdcard::fat::FatEngine;

pub(super) async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    let request_id = request.id;
    let queue_wait_ms = elapsed_since_ms_u32(request.enqueued_at_ms);
    let mut result = match split_upload_command(request.command) {
        UploadCommandGroup::Stream(stream) => {
            process_upload_stream_request(
                stream,
                queue_wait_ms,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
        UploadCommandGroup::Path(path) => {
            process_upload_path_request(
                path,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
    };
    result.request_id = request_id;
    if !sd_probe.is_initialized() {
        // Transport recovery invalidates both the probe and FatEngine's cached
        // upload state. Reset the software session so a retry can begin cleanly.
        *upload_mounted = false;
        *session = None;
        fat_engine.invalidate();
    }
    result
}

#[inline(never)]
async fn process_upload_stream_request(
    command: UploadStreamCommand,
    queue_wait_ms: u32,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    match command {
        UploadStreamCommand::Begin {
            path,
            path_len,
            expected_size,
        } => {
            handle_begin(
                SdUploadBegin {
                    path,
                    path_len,
                    expected_size,
                },
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
        UploadStreamCommand::Chunk { data_len } => {
            let handler_started_at = Instant::now();
            let mut result = handle_chunk(
                data_len,
                queue_wait_ms,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await;
            result.chunk_queue_wait_ms = queue_wait_ms;
            result.chunk_handler_ms = elapsed_ms_u32(handler_started_at);
            result.chunk_handler_done_at_ms = now_ms_u32();
            result
        }
        UploadStreamCommand::Commit => {
            handle_commit(session, sd_probe, powered, upload_mounted, fat_engine).await
        }
        UploadStreamCommand::Abort => {
            handle_abort(session, sd_probe, powered, upload_mounted, fat_engine).await
        }
    }
}

fn elapsed_since_ms_u32(started_ms: u32) -> u32 {
    let now_ms = now_ms_u32();
    now_ms.wrapping_sub(started_ms)
}

fn now_ms_u32() -> u32 {
    let now_ms = Instant::now().as_millis();
    if now_ms > u32::MAX as u64 {
        u32::MAX
    } else {
        now_ms as u32
    }
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

#[inline(never)]
async fn process_upload_path_request(
    command: UploadPathCommand,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> SdUploadResult {
    match command {
        UploadPathCommand::Mkdir { path, path_len } => {
            handle_mkdir(
                path,
                path_len,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
        UploadPathCommand::Remove { path, path_len } => {
            handle_remove(
                path,
                path_len,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
        UploadPathCommand::Stat { path, path_len } => {
            handle_stat(
                path,
                path_len,
                session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await
        }
    }
}
