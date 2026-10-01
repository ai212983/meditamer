use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration};
use sdcard::fat::FatEngine;

use super::super::mountain_read::MountainSession;
use super::super::upload::SdUploadSession;
use crate::firmware::config::{SD_REQUESTS, SD_UPLOAD_REQUESTS};
use crate::firmware::storage::ambient_assets::AmbientReadRequest;
use crate::firmware::storage::clock_assets::{receive_asset_request, AssetReadRequest};
use crate::firmware::storage::mountain_assets::MountainCommand;
use crate::firmware::types::{SdProbeDriver, SdRequest};

use super::handlers::process_upload_request_and_publish;

/// Owned intake decision. Classified first from channel-only receives; the
/// caller awaits the matching handler only after the select resolves.
///
/// Visible across the SD task (not just the intake parent) because the
/// runtime loop matches on the re-exported alias; narrowed to the task
/// rather than the whole crate.
pub(in crate::firmware::storage::sd_task) enum IntakeDecision {
    Asset(AssetReadRequest),
    AmbientAsset(AmbientReadRequest),
    MountainAsset(MountainCommand),
    Core(SdRequest),
    /// An upload was handled inline, or the idle deadline elapsed with no
    /// core traffic. The dispatcher retries the intake.
    Idle,
}

#[cfg(not(feature = "asset-upload-http"))]
pub(super) async fn receive_request_without_wifi(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    mountain_session: &mut Option<MountainSession>,
    fat_engine: &mut FatEngine,
) -> IntakeDecision {
    if *powered {
        return receive_request_without_wifi_powered(
            sd_probe,
            powered,
            upload_mounted,
            upload_session,
            mountain_session,
            fat_engine,
        )
        .await;
    }
    receive_request_without_wifi_unpowered(
        sd_probe,
        powered,
        upload_mounted,
        upload_session,
        mountain_session,
        fat_engine,
    )
    .await
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_powered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    mountain_session: &mut Option<MountainSession>,
    fat_engine: &mut FatEngine,
) -> IntakeDecision {
    match select(
        receive_asset_request(),
        select(
            crate::firmware::storage::ambient_assets::receive_asset_request(),
            select(
                crate::firmware::storage::mountain_assets::receive_asset_request(),
                select(
                    SD_UPLOAD_REQUESTS.receive(),
                    with_timeout(
                        Duration::from_millis(super::super::SD_IDLE_POWER_OFF_MS),
                        SD_REQUESTS.receive(),
                    ),
                ),
            ),
        ),
    )
    .await
    {
        Either::First(asset) => IntakeDecision::Asset(asset),
        Either::Second(Either::First(asset)) => IntakeDecision::AmbientAsset(asset),
        Either::Second(Either::Second(Either::First(asset))) => {
            IntakeDecision::MountainAsset(asset)
        }
        Either::Second(Either::Second(Either::Second(Either::First(upload_request)))) => {
            *mountain_session = None;
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await;
            IntakeDecision::Idle
        }
        Either::Second(Either::Second(Either::Second(Either::Second(Ok(request))))) => {
            IntakeDecision::Core(request)
        }
        Either::Second(Either::Second(Either::Second(Either::Second(Err(_))))) => {
            IntakeDecision::Idle
        }
    }
}

#[cfg(not(feature = "asset-upload-http"))]
async fn receive_request_without_wifi_unpowered(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    mountain_session: &mut Option<MountainSession>,
    fat_engine: &mut FatEngine,
) -> IntakeDecision {
    match select(
        receive_asset_request(),
        select(
            crate::firmware::storage::ambient_assets::receive_asset_request(),
            select(
                crate::firmware::storage::mountain_assets::receive_asset_request(),
                select(SD_UPLOAD_REQUESTS.receive(), SD_REQUESTS.receive()),
            ),
        ),
    )
    .await
    {
        Either::First(asset) => IntakeDecision::Asset(asset),
        Either::Second(Either::First(asset)) => IntakeDecision::AmbientAsset(asset),
        Either::Second(Either::Second(Either::First(asset))) => {
            IntakeDecision::MountainAsset(asset)
        }
        Either::Second(Either::Second(Either::Second(Either::First(upload_request)))) => {
            *mountain_session = None;
            process_upload_request_and_publish(
                upload_request,
                upload_session,
                sd_probe,
                powered,
                upload_mounted,
                fat_engine,
            )
            .await;
            IntakeDecision::Idle
        }
        Either::Second(Either::Second(Either::Second(Either::Second(request)))) => {
            IntakeDecision::Core(request)
        }
    }
}
