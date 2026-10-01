use super::*;

pub(super) async fn disconnect_with_timeout(controller: &mut WifiController<'_>, context: &str) {
    log_radio_mem_diag_with_trigger("recover_disconnect_before", context);
    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_disconnect_async(controller),
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
    controller: &mut WifiController<'_>,
    context: &str,
) {
    disconnect_with_timeout(controller, context).await;
    match with_timeout(
        Duration::from_millis(WIFI_DRIVER_CONTROL_TIMEOUT_MS),
        wifi_stop_async(controller),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            diag_reassoc!("upload_http: {} stop err={:?}", context, err);
        }
        Err(_) => {
            diag_reassoc!(
                "upload_http: {} stop timeout={}ms",
                context,
                WIFI_DRIVER_CONTROL_TIMEOUT_MS
            );
        }
    }
}
