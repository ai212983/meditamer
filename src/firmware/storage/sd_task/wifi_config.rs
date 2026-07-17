use sdcard::fat::{FatEngine, FatEngineError, FatPayloadId, FatRequest, FatResult, SdFatError};

use super::super::super::types::{
    SdProbeDriver, SdUploadResultCode, WifiConfigRequest, WifiConfigResponse, WifiConfigResultCode,
    WifiCredentials, WIFI_CONFIG_FILE_MAX, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
};
use super::upload::{ensure_upload_ready, SdUploadSession};
use super::{WIFI_CONFIG_DIR, WIFI_CONFIG_PATH};

pub(super) async fn process_wifi_config_request(
    request: WifiConfigRequest,
    session: &Option<SdUploadSession>,
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    fat_engine: &mut FatEngine,
) -> WifiConfigResponse {
    if session.is_some() {
        return wifi_config_response(false, WifiConfigResultCode::Busy, None);
    }

    if let Err(code) = ensure_upload_ready(sd_probe, powered, upload_mounted).await {
        return wifi_config_response(false, map_upload_ready_error(code), None);
    }

    match request {
        WifiConfigRequest::Load => {
            let mut raw = [0u8; WIFI_CONFIG_FILE_MAX];
            let (path, path_len) = fat_path(WIFI_CONFIG_PATH);
            let result = super::engine_driver::run_fat_request(
                FatRequest::Read {
                    path,
                    path_len,
                    output: FatPayloadId::Primary,
                    output_capacity: raw.len() as u32,
                },
                sd_probe,
                fat_engine,
                &[],
                &mut raw,
            )
            .await;
            match result {
                FatResult::Read { bytes } => match parse_wifi_config_file(&raw[..bytes as usize]) {
                    Ok(credentials) => {
                        wifi_config_response(true, WifiConfigResultCode::Ok, Some(credentials))
                    }
                    Err(_) => wifi_config_response(false, WifiConfigResultCode::InvalidData, None),
                },
                FatResult::Error(FatEngineError::Fat(SdFatError::NotFound)) => {
                    wifi_config_response(false, WifiConfigResultCode::NotFound, None)
                }
                result => {
                    wifi_config_response(false, map_fat_result_to_wifi_config_code(&result), None)
                }
            }
        }
        WifiConfigRequest::Store { credentials } => {
            let (dir, dir_len) = fat_path(WIFI_CONFIG_DIR);
            let mut no_output = [];
            let mkdir = super::engine_driver::run_fat_request(
                FatRequest::Mkdir {
                    path: dir,
                    path_len: dir_len,
                },
                sd_probe,
                fat_engine,
                &[],
                &mut no_output,
            )
            .await;
            match mkdir {
                FatResult::Done
                | FatResult::Error(FatEngineError::Fat(SdFatError::AlreadyExists)) => {}
                result => {
                    return wifi_config_response(
                        false,
                        map_fat_result_to_wifi_config_code(&result),
                        None,
                    );
                }
            }

            let mut encoded = [0u8; WIFI_CONFIG_FILE_MAX];
            let encoded_len = match encode_wifi_config_file(credentials, &mut encoded) {
                Ok(len) => len,
                Err(_) => {
                    return wifi_config_response(false, WifiConfigResultCode::InvalidData, None);
                }
            };

            let (path, path_len) = fat_path(WIFI_CONFIG_PATH);
            let result = super::engine_driver::run_fat_request(
                FatRequest::Write {
                    path,
                    path_len,
                    input: FatPayloadId::Primary,
                    input_len: encoded_len as u32,
                },
                sd_probe,
                fat_engine,
                &encoded[..encoded_len],
                &mut no_output,
            )
            .await;
            match result {
                FatResult::Done => wifi_config_response(true, WifiConfigResultCode::Ok, None),
                result => {
                    wifi_config_response(false, map_fat_result_to_wifi_config_code(&result), None)
                }
            }
        }
    }
}

#[inline(never)]
fn map_upload_ready_error(code: SdUploadResultCode) -> WifiConfigResultCode {
    match code {
        SdUploadResultCode::PowerOnFailed => WifiConfigResultCode::PowerOnFailed,
        SdUploadResultCode::InitFailed => WifiConfigResultCode::InitFailed,
        _ => WifiConfigResultCode::OperationFailed,
    }
}

#[inline(never)]
fn map_fat_result_to_wifi_config_code(result: &FatResult) -> WifiConfigResultCode {
    match result {
        FatResult::Error(FatEngineError::Fat(SdFatError::NotFound)) => {
            WifiConfigResultCode::NotFound
        }
        FatResult::Error(FatEngineError::Fat(
            SdFatError::InvalidPath | SdFatError::BufferTooSmall { .. },
        )) => WifiConfigResultCode::InvalidData,
        _ => WifiConfigResultCode::OperationFailed,
    }
}

fn fat_path(value: &str) -> ([u8; sdcard::SD_PATH_MAX], u8) {
    let bytes = value.as_bytes();
    let mut path = [0u8; sdcard::SD_PATH_MAX];
    path[..bytes.len()].copy_from_slice(bytes);
    (path, bytes.len() as u8)
}

#[inline(never)]
fn wifi_config_response(
    ok: bool,
    code: WifiConfigResultCode,
    credentials: Option<WifiCredentials>,
) -> WifiConfigResponse {
    WifiConfigResponse {
        ok,
        code,
        credentials,
    }
}

#[inline(never)]
fn encode_wifi_config_file(
    credentials: WifiCredentials,
    out: &mut [u8; WIFI_CONFIG_FILE_MAX],
) -> Result<usize, ()> {
    let ssid_len = credentials.ssid_len as usize;
    let password_len = credentials.password_len as usize;
    if ssid_len == 0 || ssid_len > WIFI_SSID_MAX || password_len > WIFI_PASSWORD_MAX {
        return Err(());
    }

    if contains_wifi_config_line_break(&credentials.ssid[..ssid_len])
        || contains_wifi_config_line_break(&credentials.password[..password_len])
    {
        return Err(());
    }

    let mut cursor = 0usize;
    cursor = write_ascii(out, cursor, b"ssid=").ok_or(())?;
    cursor = write_ascii(out, cursor, &credentials.ssid[..ssid_len]).ok_or(())?;
    cursor = write_ascii(out, cursor, b"\npassword=").ok_or(())?;
    cursor = write_ascii(out, cursor, &credentials.password[..password_len]).ok_or(())?;
    cursor = write_ascii(out, cursor, b"\n").ok_or(())?;
    Ok(cursor)
}

fn contains_wifi_config_line_break(bytes: &[u8]) -> bool {
    bytes.iter().any(|byte| matches!(*byte, b'\n' | b'\r'))
}

fn parse_wifi_config_file(buf: &[u8]) -> Result<WifiCredentials, ()> {
    let mut ssid: Option<&[u8]> = None;
    let mut password: Option<&[u8]> = None;

    for raw_line in buf.split(|b| *b == b'\n') {
        let line = trim_ascii_line(raw_line);
        if line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix(b"ssid=") {
            ssid = Some(value);
        } else if let Some(value) = line.strip_prefix(b"password=") {
            password = Some(value);
        }
    }

    let ssid = ssid.ok_or(())?;
    if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX {
        return Err(());
    }
    let password = password.unwrap_or(&[]);
    if password.len() > WIFI_PASSWORD_MAX {
        return Err(());
    }

    let mut credentials = WifiCredentials {
        ssid: [0u8; WIFI_SSID_MAX],
        ssid_len: ssid.len() as u8,
        password: [0u8; WIFI_PASSWORD_MAX],
        password_len: password.len() as u8,
    };
    credentials.ssid[..ssid.len()].copy_from_slice(ssid);
    credentials.password[..password.len()].copy_from_slice(password);
    Ok(credentials)
}

fn write_ascii(dst: &mut [u8], cursor: usize, bytes: &[u8]) -> Option<usize> {
    let next = cursor.checked_add(bytes.len())?;
    if next > dst.len() {
        return None;
    }
    dst[cursor..next].copy_from_slice(bytes);
    Some(next)
}

fn trim_ascii_line(mut line: &[u8]) -> &[u8] {
    while matches!(line.last(), Some(b'\r' | b' ' | b'\t')) {
        line = &line[..line.len().saturating_sub(1)];
    }
    while matches!(line.first(), Some(b' ' | b'\t')) {
        line = &line[1..];
    }
    line
}
