use super::super::super::super::types::{SdProbeDriver, SdUploadRequest, SdUploadResult};
use super::path_ops::{handle_mkdir, handle_remove, handle_stat};
use super::stream::{handle_abort, handle_begin, handle_chunk, handle_commit};
use super::types::{
    split_upload_command, SdUploadSession, UploadCommandGroup, UploadPathCommand,
    UploadStreamCommand,
};

pub(super) async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    match split_upload_command(request.command) {
        UploadCommandGroup::Stream(stream) => {
            process_upload_stream_request(stream, session, sd_probe, powered, upload_mounted).await
        }
        UploadCommandGroup::Path(path) => {
            process_upload_path_request(path, session, sd_probe, powered, upload_mounted).await
        }
    }
}

#[inline(never)]
async fn process_upload_stream_request(
    command: UploadStreamCommand,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    match command {
        UploadStreamCommand::Begin {
            path,
            path_len,
            expected_size,
        } => {
            handle_begin(
                path,
                path_len,
                expected_size,
                session,
                sd_probe,
                powered,
                upload_mounted,
            )
            .await
        }
        UploadStreamCommand::Chunk { data_len } => {
            handle_chunk(data_len, session, sd_probe, powered, upload_mounted).await
        }
        UploadStreamCommand::Commit => {
            handle_commit(session, sd_probe, powered, upload_mounted).await
        }
        UploadStreamCommand::Abort => {
            handle_abort(session, sd_probe, powered, upload_mounted).await
        }
    }
}

#[inline(never)]
async fn process_upload_path_request(
    command: UploadPathCommand,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    match command {
        UploadPathCommand::Mkdir { path, path_len } => {
            handle_mkdir(path, path_len, session, sd_probe, powered, upload_mounted).await
        }
        UploadPathCommand::Remove { path, path_len } => {
            handle_remove(path, path_len, session, sd_probe, powered, upload_mounted).await
        }
        UploadPathCommand::Stat { path, path_len } => {
            handle_stat(path, path_len, session, sd_probe, powered, upload_mounted).await
        }
    }
}
