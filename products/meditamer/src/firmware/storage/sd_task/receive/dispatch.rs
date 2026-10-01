#[cfg(not(feature = "asset-upload-http"))]
use super::nonwifi::{receive_request_without_wifi, IntakeDecision};
use sdcard::fat::FatEngine;

use super::super::super::super::types::SdProbeDriver;
use super::super::mountain_read::MountainSession;
use super::super::upload::SdUploadSession;

#[cfg(not(feature = "asset-upload-http"))]
pub(in crate::firmware::storage::sd_task) use super::nonwifi::IntakeDecision as CoreIntake;

#[cfg(not(feature = "asset-upload-http"))]
pub(in crate::firmware::storage::sd_task) async fn receive_core_request(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    upload_mounted: &mut bool,
    upload_session: &mut Option<SdUploadSession>,
    mountain_session: &mut Option<MountainSession>,
    fat_engine: &mut FatEngine,
) -> CoreIntake {
    loop {
        match receive_request_without_wifi(
            sd_probe,
            powered,
            upload_mounted,
            upload_session,
            mountain_session,
            fat_engine,
        )
        .await
        {
            IntakeDecision::Idle => {}
            decision => return decision,
        }
    }
}
