#[embassy_executor::task]
pub(crate) async fn sd_task(mut sd_probe: SdProbeDriver) {
    let boot_started_at = Instant::now();
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

    #[cfg(feature = "asset-upload-http")]
    if crate::firmware::storage::transfer_buffers::lock_upload_chunk_buffer()
        .await
        .is_err()
    {
        esp_println::println!("sdtask: upload_chunk_buffer_prewarm_failed");
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
        if upload_session.is_some() {
            if !service_mode::upload_transfers_enabled() {
                telemetry::record_sd_upload_session_mode_off_abort();
                let result = abort_active_upload_session(
                    &mut upload_session,
                    &mut sd_probe,
                    &mut powered,
                    &mut upload_mounted,
                )
                .await;
                publish_upload_result(result);
                continue;
            }

            if let Some(last_activity_at) = upload::active_session_last_activity(&upload_session) {
                let idle_ms = duration_ms_since(last_activity_at);
                if idle_ms >= SD_UPLOAD_SESSION_IDLE_ABORT_MS {
                    telemetry::record_sd_upload_session_timeout_abort();
                    esp_println::println!(
                        "sdtask: upload_session_idle_abort idle_ms={} threshold_ms={}",
                        idle_ms,
                        SD_UPLOAD_SESSION_IDLE_ABORT_MS
                    );
                    let result = abort_active_upload_session(
                        &mut upload_session,
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await;
                    publish_upload_result(result);
                    continue;
                }
            }
        }

        #[cfg(feature = "asset-upload-http")]
        let request = if powered {
            match select3(
                WIFI_CONFIG_REQUESTS.receive(),
                SD_UPLOAD_REQUESTS.receive(),
                with_timeout(
                    Duration::from_millis(SD_IDLE_POWER_OFF_MS),
                    SD_REQUESTS.receive(),
                ),
            )
            .await
            {
                Either3::First(config_request) => {
                    if !service_mode::upload_transfers_enabled() {
                        publish_wifi_config_response(disabled_wifi_config_response());
                        continue;
                    }
                    if let Err(code) = ensure_upload_storage_ready(
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await
                    {
                        publish_wifi_config_response(wifi_config_error_response(code));
                        continue;
                    }
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
                Either3::Second(upload_request) => {
                    if !service_mode::upload_transfers_enabled() {
                        publish_upload_result(disabled_upload_result());
                        continue;
                    }
                    if let Err(code) = ensure_upload_storage_ready(
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await
                    {
                        publish_upload_result(SdUploadResult {
                            ok: false,
                            code,
                            bytes_written: 0,
                            chunk_queue_wait_ms: 0,
                            chunk_handler_ms: 0,
                        });
                        continue;
                    }
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
                Either3::Third(result) => result.ok(),
            }
        } else {
            match select3(
                WIFI_CONFIG_REQUESTS.receive(),
                SD_UPLOAD_REQUESTS.receive(),
                SD_REQUESTS.receive(),
            )
            .await
            {
                Either3::First(config_request) => {
                    if !service_mode::upload_transfers_enabled() {
                        publish_wifi_config_response(disabled_wifi_config_response());
                        continue;
                    }
                    if let Err(code) = ensure_upload_storage_ready(
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await
                    {
                        publish_wifi_config_response(wifi_config_error_response(code));
                        continue;
                    }
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
                Either3::Second(upload_request) => {
                    if !service_mode::upload_transfers_enabled() {
                        publish_upload_result(disabled_upload_result());
                        continue;
                    }
                    if let Err(code) = ensure_upload_storage_ready(
                        &mut sd_probe,
                        &mut powered,
                        &mut upload_mounted,
                    )
                    .await
                    {
                        publish_upload_result(SdUploadResult {
                            ok: false,
                            code,
                            bytes_written: 0,
                            chunk_queue_wait_ms: 0,
                            chunk_handler_ms: 0,
                        });
                        continue;
                    }
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
                Either3::Third(request) => Some(request),
            }
        };

        #[cfg(not(feature = "asset-upload-http"))]
        let request = receive_core_request(
            &mut sd_probe,
            &mut powered,
            &mut upload_mounted,
            &mut upload_session,
        )
        .await;

        let Some(request) = request else {
            #[cfg(feature = "asset-upload-http")]
            if upload_session.is_some() {
                // Keep SD online during an active upload session; stale sessions are cleaned up
                // by the idle-abort/mode-off checks at the top of this loop.
                continue;
            }
            if powered && duration_ms_since(boot_started_at) < SD_BOOT_POWER_OFF_GRACE_MS as u32 {
                continue;
            }
            if powered && !request_sd_power(SdPowerRequest::Off).await {
                esp_println::println!("sdtask: idle_power_off_failed");
            }
            powered = false;
            sd_probe.invalidate();
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
            sd_probe.invalidate();
            upload_mounted = false;
        }
    }
}
