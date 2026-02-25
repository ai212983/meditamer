#[cfg(not(feature = "asset-upload-http"))]
use embassy_futures::select::{select3, Either3};
#[cfg(feature = "asset-upload-http")]
use embassy_futures::select::{select4, Either4};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use sdcard::runtime as sd_ops;

#[cfg(feature = "asset-upload-http")]
use super::super::config::WIFI_CONFIG_REQUESTS;
use super::super::{
    config::{
        SD_ASSET_READ_REQUESTS, SD_POWER_REQUESTS, SD_POWER_RESPONSES, SD_REQUESTS,
        SD_UPLOAD_REQUESTS,
    },
    types::{SdCommand, SdPowerRequest, SdProbeDriver, SdRequest},
};

mod asset_read;
mod dispatch;
mod logging;
mod power;
#[cfg(test)]
mod tests;
mod upload;
#[cfg(feature = "asset-upload-http")]
mod wifi_config;

use asset_read::process_asset_read_request;
use dispatch::process_request;
#[cfg(feature = "asset-upload-http")]
use logging::publish_wifi_config_response;
use logging::{
    publish_asset_read_response, publish_result, publish_upload_result, sd_power_action_label,
};
pub(super) use power::{duration_ms_since, failure_backoff_ms, request_sd_power};
use upload::{process_upload_request, SdUploadSession};
#[cfg(feature = "asset-upload-http")]
use wifi_config::process_wifi_config_request;

const SD_IDLE_POWER_OFF_MS: u64 = 1_500;
const SD_RETRY_MAX_ATTEMPTS: u8 = 3;
const SD_RETRY_DELAY_MS: u64 = 120;
const SD_BACKOFF_BASE_MS: u64 = 300;
const SD_BACKOFF_MAX_MS: u64 = 2_400;
const SD_POWER_RESPONSE_TIMEOUT_MS: u64 = 1_000;
const SD_UPLOAD_TMP_SUFFIX: &[u8] = b".part";
const SD_UPLOAD_PATH_BUF_MAX: usize = 72;
const SD_UPLOAD_ROOT: &str = "/assets";
#[cfg(feature = "asset-upload-http")]
const WIFI_CONFIG_DIR: &str = "/config";
#[cfg(feature = "asset-upload-http")]
const WIFI_CONFIG_PATH: &str = "/config/wifi.cfg";

#[embassy_executor::task]
pub(crate) async fn sd_task(mut sd_probe: SdProbeDriver) {
    let mut powered = false;
    let mut upload_mounted = false;
    let mut upload_session: Option<SdUploadSession> = None;
    let mut no_power = |_action: sd_ops::SdPowerAction| -> Result<(), ()> { Ok(()) };
    let mut consecutive_failures = 0u8;
    let mut backoff_until: Option<Instant> = None;

    // Keep boot probe behavior, but now report completion through result channel.
    let boot_req = SdRequest {
        id: 0,
        command: SdCommand::Probe,
    };
    let boot_result = process_request(boot_req, &mut sd_probe, &mut powered, &mut no_power).await;
    publish_result(boot_result);
    if !boot_result.ok {
        consecutive_failures = 1;
        backoff_until = Some(Instant::now() + Duration::from_millis(failure_backoff_ms(1)));
    }

    loop {
        if let Some(until) = backoff_until {
            let now = Instant::now();
            if now < until {
                Timer::after(until.saturating_duration_since(now)).await;
            }
            backoff_until = None;
        }

        #[cfg(feature = "asset-upload-http")]
        let request = if powered {
            match select4(
                WIFI_CONFIG_REQUESTS.receive(),
                SD_UPLOAD_REQUESTS.receive(),
                SD_ASSET_READ_REQUESTS.receive(),
                with_timeout(
                    Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                    SD_REQUESTS.receive(),
                ),
            )
            .await
            {
                Either4::First(config_request) => {
                    let response = process_wifi_config_request(
                        config_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_wifi_config_response(response);
                    continue;
                }
                Either4::Second(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either4::Third(asset_request) => {
                    let response = process_asset_read_request(
                        asset_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_asset_read_response(response);
                    continue;
                }
                Either4::Fourth(result) => result.ok(),
            }
        } else {
            match select4(
                WIFI_CONFIG_REQUESTS.receive(),
                SD_UPLOAD_REQUESTS.receive(),
                SD_ASSET_READ_REQUESTS.receive(),
                SD_REQUESTS.receive(),
            )
            .await
            {
                Either4::First(config_request) => {
                    let response = process_wifi_config_request(
                        config_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_wifi_config_response(response);
                    continue;
                }
                Either4::Second(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either4::Third(asset_request) => {
                    let response = process_asset_read_request(
                        asset_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_asset_read_response(response);
                    continue;
                }
                Either4::Fourth(request) => Some(request),
            }
        };

        #[cfg(not(feature = "asset-upload-http"))]
        let request = if powered {
            match select3(
                SD_UPLOAD_REQUESTS.receive(),
                SD_ASSET_READ_REQUESTS.receive(),
                with_timeout(
                    Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                    SD_REQUESTS.receive(),
                ),
            )
            .await
            {
                Either3::First(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either3::Second(asset_request) => {
                    let response = process_asset_read_request(
                        asset_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_asset_read_response(response);
                    continue;
                }
                Either3::Third(result) => result.ok(),
            }
        } else {
            match select3(
                SD_UPLOAD_REQUESTS.receive(),
                SD_ASSET_READ_REQUESTS.receive(),
                SD_REQUESTS.receive(),
            )
            .await
            {
                Either3::First(upload_request) => {
                    let result = process_upload_request(
                        upload_request,
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
                Either3::Second(asset_request) => {
                    let response = process_asset_read_request(
                        asset_request,
                        &upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_asset_read_response(response);
                    continue;
                }
                Either3::Third(request) => Some(request),
            }
        };

        let Some(request) = request else {
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: idle_power_off_failed");
            }
            powered = false;
            upload_mounted = false;
            continue;
        };

        let result = process_request(request, &mut sd_probe, &mut powered, &mut no_power).await;
        publish_result(result);

        if result.ok {
            consecutive_failures = 0;
            backoff_until = None;
        } else {
            consecutive_failures = consecutive_failures.saturating_add(1).min(8);
            let backoff_ms = failure_backoff_ms(consecutive_failures);
            backoff_until = Some(Instant::now() + Duration::from_millis(backoff_ms));
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: fail_power_off_failed");
            }
            powered = false;
            upload_mounted = false;
        }
    }
}
