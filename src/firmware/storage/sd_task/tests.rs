use super::{
    asset_read::parse_asset_path,
    dispatch::{sd_command_kind, sd_result_should_retry},
    failure_backoff_ms,
    upload::parse_upload_path,
    SD_BACKOFF_BASE_MS, SD_BACKOFF_MAX_MS,
};
use crate::firmware::types::{
    SdAssetReadResultCode, SdCommand, SdCommandKind, SdResultCode, SdUploadResultCode, SD_PATH_MAX,
};

#[test]
fn backoff_grows_and_clamps() {
    assert_eq!(failure_backoff_ms(0), SD_BACKOFF_BASE_MS);
    assert_eq!(failure_backoff_ms(1), SD_BACKOFF_BASE_MS);
    assert_eq!(failure_backoff_ms(2), SD_BACKOFF_BASE_MS * 2);
    assert_eq!(failure_backoff_ms(3), SD_BACKOFF_BASE_MS * 4);
    assert_eq!(failure_backoff_ms(8), SD_BACKOFF_MAX_MS);
    assert_eq!(failure_backoff_ms(32), SD_BACKOFF_MAX_MS);
}

#[test]
fn command_kind_mapping_is_stable() {
    assert_eq!(sd_command_kind(SdCommand::Probe), SdCommandKind::Probe);
    assert_eq!(
        sd_command_kind(SdCommand::RwVerify { lba: 1 }),
        SdCommandKind::RwVerify
    );
    assert_eq!(
        sd_command_kind(SdCommand::FatStat {
            path: [0; SD_PATH_MAX],
            path_len: 0
        }),
        SdCommandKind::FatStat
    );
    assert_eq!(
        sd_command_kind(SdCommand::FatTruncate {
            path: [0; SD_PATH_MAX],
            path_len: 0,
            size: 0
        }),
        SdCommandKind::FatTruncate
    );
}

#[test]
fn retry_policy_matches_result_codes() {
    assert!(sd_result_should_retry(SdResultCode::PowerOnFailed));
    assert!(sd_result_should_retry(SdResultCode::InitFailed));
    assert!(sd_result_should_retry(SdResultCode::OperationFailed));
    assert!(!sd_result_should_retry(SdResultCode::InvalidPath));
    assert!(!sd_result_should_retry(SdResultCode::NotFound));
    assert!(!sd_result_should_retry(SdResultCode::VerifyMismatch));
    assert!(!sd_result_should_retry(SdResultCode::PowerOffFailed));
    assert!(!sd_result_should_retry(SdResultCode::RefusedLba0));
}

#[test]
fn upload_path_validation_allows_assets_root_and_children() {
    assert_eq!(parse_upload_path_bytes("/assets"), Ok("/assets"));
    assert_eq!(
        parse_upload_path_bytes("/assets/file.bin"),
        Ok("/assets/file.bin")
    );
}

#[test]
fn upload_path_validation_rejects_outside_root_and_dot_segments() {
    assert_eq!(
        parse_upload_path_bytes("/config/wifi.cfg"),
        Err(SdUploadResultCode::InvalidPath)
    );
    assert_eq!(
        parse_upload_path_bytes("/assets/../config"),
        Err(SdUploadResultCode::InvalidPath)
    );
    assert_eq!(
        parse_upload_path_bytes("/assets/./file.bin"),
        Err(SdUploadResultCode::InvalidPath)
    );
}

#[test]
fn upload_path_validation_rejects_control_characters() {
    assert_eq!(
        parse_upload_path_bytes("/assets/file\n.bin"),
        Err(SdUploadResultCode::InvalidPath)
    );
}

#[test]
fn asset_path_validation_allows_assets_root_and_children() {
    assert_eq!(parse_asset_path_bytes("/assets"), Ok("/assets"));
    assert_eq!(
        parse_asset_path_bytes("/assets/raw/fonts/pirata_clock/digit_0_mono1.raw"),
        Ok("/assets/raw/fonts/pirata_clock/digit_0_mono1.raw")
    );
}

#[test]
fn asset_path_validation_rejects_outside_root_and_dot_segments() {
    assert_eq!(
        parse_asset_path_bytes("/config/wifi.cfg"),
        Err(SdAssetReadResultCode::InvalidPath)
    );
    assert_eq!(
        parse_asset_path_bytes("/assets/../config"),
        Err(SdAssetReadResultCode::InvalidPath)
    );
    assert_eq!(
        parse_asset_path_bytes("/assets/./font.raw"),
        Err(SdAssetReadResultCode::InvalidPath)
    );
}

fn parse_upload_path_bytes(path: &str) -> Result<&str, SdUploadResultCode> {
    let bytes = path.as_bytes();
    parse_upload_path(bytes, bytes.len() as u8)
}

fn parse_asset_path_bytes(path: &str) -> Result<&str, SdAssetReadResultCode> {
    let bytes = path.as_bytes();
    parse_asset_path(bytes, bytes.len() as u8)
}
