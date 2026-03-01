use super::*;
pub(super) async fn disconnect_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    log_radio_mem_diag_with_trigger("recover_disconnect_before", context);
    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        controller.disconnect_async(),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            diag_reassoc!("upload_http: {} disconnect err={:?}", context, err);
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: {} disconnect timeout={}ms",
                context,
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
    log_radio_mem_diag_with_trigger("recover_disconnect_after", context);
}

pub(super) async fn disconnect_and_stop_with_timeout(
    controller: &mut WifiController<'static>,
    context: &str,
) {
    disconnect_with_timeout(controller, context).await;
    let mut stop_attempt = 0u8;
    loop {
        log_radio_mem_diag_with_trigger("recover_stop_before", context);
        match with_timeout(
            Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
            controller.stop_async(),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                diag_reassoc!(
                    "upload_http: {} stop err={:?} attempt={}",
                    context,
                    err,
                    stop_attempt + 1
                );
            }
            Err(_) => {
                diag_reassoc!(
                    "upload_http: {} stop timeout={}ms attempt={}",
                    context,
                    WIFI_DRIVER_CONTROL_TIMEOUT_MS,
                    stop_attempt + 1
                );
            }
        }
        log_radio_mem_diag_with_trigger("recover_stop_after", context);
        match controller.is_started() {
            Ok(false) => break,
            Ok(true) => {
                if stop_attempt >= WIFI_DRIVER_STOP_RETRIES {
                    diag_reassoc!(
                        "upload_http: {} stop retries exhausted; controller still started",
                        context
                    );
                    break;
                }
            }
            Err(err) => {
                diag_reassoc!(
                    "upload_http: {} is_started check err={:?} after stop",
                    context,
                    err
                );
                break;
            }
        }
        stop_attempt = stop_attempt.saturating_add(1);
        Timer::after(Duration::from_millis(WIFI_DRIVER_STOP_RETRY_BACKOFF_MS)).await;
    }
}
