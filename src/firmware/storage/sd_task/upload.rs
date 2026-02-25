use sdcard::fat;

use super::super::super::types::{
    SdProbeDriver, SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode,
    SD_UPLOAD_CHUNK_MAX,
};
use super::super::transfer_buffers;
use super::{SD_UPLOAD_PATH_BUF_MAX, SD_UPLOAD_TMP_SUFFIX};

mod helpers;

use helpers::{map_fat_error_to_upload_code, upload_result};

pub(super) struct SdUploadSession {
    pub(super) final_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) final_path_len: u8,
    pub(super) temp_path: [u8; SD_UPLOAD_PATH_BUF_MAX],
    pub(super) temp_path_len: u8,
    pub(super) expected_size: u32,
    pub(super) bytes_written: u32,
}

pub(super) async fn process_upload_request(
    request: SdUploadRequest,
    session: &mut Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> SdUploadResult {
    let command = request.command;
    match command {
        SdUploadCommand::Begin {
            path,
            path_len,
            expected_size,
        } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            let final_path = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };
            let final_path_bytes = final_path.as_bytes();

            if final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len() > SD_UPLOAD_PATH_BUF_MAX {
                return upload_result(false, SdUploadResultCode::InvalidPath, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let mut temp_path = [0u8; SD_UPLOAD_PATH_BUF_MAX];
            temp_path[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
            temp_path[final_path_bytes.len()..final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len()]
                .copy_from_slice(SD_UPLOAD_TMP_SUFFIX);
            let temp_len = final_path_bytes.len() + SD_UPLOAD_TMP_SUFFIX.len();
            let temp_path_str = match core::str::from_utf8(&temp_path[..temp_len]) {
                Ok(path) => path,
                Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
            };

            match fat::remove(sd_probe, temp_path_str).await {
                Ok(()) => {}
                Err(fat::SdFatError::NotFound) => {}
                Err(err) => {
                    return upload_result(false, map_fat_error_to_upload_code(&err), 0);
                }
            }

            if let Err(err) = fat::write_file(sd_probe, temp_path_str, &[]).await {
                return upload_result(false, map_fat_error_to_upload_code(&err), 0);
            }

            let mut final_path_buf = [0u8; SD_UPLOAD_PATH_BUF_MAX];
            final_path_buf[..final_path_bytes.len()].copy_from_slice(final_path_bytes);
            *session = Some(SdUploadSession {
                final_path: final_path_buf,
                final_path_len: final_path_bytes.len() as u8,
                temp_path,
                temp_path_len: temp_len as u8,
                expected_size,
                bytes_written: 0,
            });
            upload_result(true, SdUploadResultCode::Ok, 0)
        }
        SdUploadCommand::Chunk { data_len } => {
            let Some(active) = session.as_mut() else {
                return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
            };
            let data_len = (data_len as usize).min(SD_UPLOAD_CHUNK_MAX);
            if data_len == 0 {
                return upload_result(true, SdUploadResultCode::Ok, active.bytes_written);
            }

            let Some(next_bytes_written) = active.bytes_written.checked_add(data_len as u32) else {
                return upload_result(
                    false,
                    SdUploadResultCode::SizeMismatch,
                    active.bytes_written,
                );
            };
            if next_bytes_written > active.expected_size {
                return upload_result(
                    false,
                    SdUploadResultCode::SizeMismatch,
                    active.bytes_written,
                );
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            let mut chunk_data = match transfer_buffers::lock_upload_chunk_buffer().await {
                Ok(buffer) => buffer,
                Err(_) => {
                    return upload_result(
                        false,
                        SdUploadResultCode::OperationFailed,
                        active.bytes_written,
                    );
                }
            };
            if let Err(err) = fat::append_file(
                sd_probe,
                temp_path_str,
                &chunk_data.as_mut_slice()[..data_len],
            )
            .await
            {
                return upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                );
            }
            active.bytes_written = next_bytes_written;
            upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
        }
        SdUploadCommand::Commit => {
            let Some(active) = session.as_mut() else {
                return upload_result(false, SdUploadResultCode::SessionNotActive, 0);
            };
            if active.bytes_written != active.expected_size {
                return upload_result(
                    false,
                    SdUploadResultCode::SizeMismatch,
                    active.bytes_written,
                );
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            let final_path_str =
                match core::str::from_utf8(&active.final_path[..active.final_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };

            match fat::remove(sd_probe, final_path_str).await {
                Ok(()) => {}
                Err(fat::SdFatError::NotFound) => {}
                Err(err) => {
                    return upload_result(
                        false,
                        map_fat_error_to_upload_code(&err),
                        active.bytes_written,
                    );
                }
            }

            if let Err(err) = fat::rename(sd_probe, temp_path_str, final_path_str).await {
                return upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                );
            }
            let bytes_written = active.bytes_written;
            *session = None;
            upload_result(true, SdUploadResultCode::Ok, bytes_written)
        }
        SdUploadCommand::Abort => {
            let Some(active) = session.take() else {
                return upload_result(true, SdUploadResultCode::Ok, 0);
            };

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, active.bytes_written);
            }

            let temp_path_str =
                match core::str::from_utf8(&active.temp_path[..active.temp_path_len as usize]) {
                    Ok(path) => path,
                    Err(_) => return upload_result(false, SdUploadResultCode::InvalidPath, 0),
                };
            match fat::remove(sd_probe, temp_path_str).await {
                Ok(()) | Err(fat::SdFatError::NotFound) => {
                    upload_result(true, SdUploadResultCode::Ok, active.bytes_written)
                }
                Err(err) => upload_result(
                    false,
                    map_fat_error_to_upload_code(&err),
                    active.bytes_written,
                ),
            }
        }
        SdUploadCommand::Mkdir { path, path_len } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let path_str = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };

            match fat::mkdir(sd_probe, path_str).await {
                Ok(()) | Err(fat::SdFatError::AlreadyExists) => {
                    upload_result(true, SdUploadResultCode::Ok, 0)
                }
                Err(err) => upload_result(false, map_fat_error_to_upload_code(&err), 0),
            }
        }
        SdUploadCommand::Remove { path, path_len } => {
            if session.is_some() {
                return upload_result(false, SdUploadResultCode::Busy, 0);
            }

            if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
                return upload_result(false, code, 0);
            }

            let path_str = match parse_upload_path(&path, path_len) {
                Ok(path) => path,
                Err(code) => return upload_result(false, code, 0),
            };

            match fat::remove(sd_probe, path_str).await {
                Ok(()) | Err(fat::SdFatError::NotFound) => {
                    upload_result(true, SdUploadResultCode::Ok, 0)
                }
                Err(err) => upload_result(false, map_fat_error_to_upload_code(&err), 0),
            }
        }
    }
}

pub(super) async fn ensure_upload_ready(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
) -> Result<(), SdUploadResultCode> {
    helpers::ensure_upload_ready(sd_probe, powered, upload_mounted).await
}

pub(super) fn parse_upload_path(path: &[u8], path_len: u8) -> Result<&str, SdUploadResultCode> {
    helpers::parse_upload_path(path, path_len)
}
