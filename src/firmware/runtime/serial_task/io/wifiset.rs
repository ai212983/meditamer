use core::fmt::Write;

use embassy_time::{with_timeout, Duration};

use crate::firmware::{
    config::{
        WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES, WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
        WIFI_CREDENTIALS_UPDATES,
    },
    touch::debug_log::uart_write_all,
    types::{WifiConfigRequest, WifiCredentials},
};

use super::super::labels::wifi_config_result_code_label;
use super::SerialUart;

pub(crate) async fn run_wifiset_command(uart: &mut SerialUart, credentials: WifiCredentials) {
    while WIFI_CREDENTIALS_UPDATES.try_receive().is_ok() {}
    while WIFI_CONFIG_RESPONSES.try_receive().is_ok() {}

    if WIFI_CREDENTIALS_UPDATES.try_send(credentials).is_err() {
        let _ = uart_write_all(uart, b"WIFISET BUSY\r\n").await;
        return;
    }

    WIFI_CONFIG_REQUESTS
        .send(WifiConfigRequest::Store { credentials })
        .await;

    match with_timeout(
        Duration::from_millis(WIFI_CONFIG_RESPONSE_TIMEOUT_MS),
        WIFI_CONFIG_RESPONSES.receive(),
    )
    .await
    {
        Ok(result) if result.ok => {
            let _ = uart_write_all(uart, b"WIFISET OK\r\n").await;
        }
        Ok(result) => {
            // Runtime credentials update already succeeded; persistence to SD is best-effort.
            let _ = uart_write_all(uart, b"WIFISET OK\r\n").await;
            let mut warn = heapless::String::<112>::new();
            let _ = write!(
                &mut warn,
                "WIFISET WARN persist={}\r\n",
                wifi_config_result_code_label(result.code)
            );
            let _ = uart_write_all(uart, warn.as_bytes()).await;
        }
        Err(_) => {
            // Keep volatile credentials active even when SD persistence times out.
            let _ = uart_write_all(uart, b"WIFISET OK\r\n").await;
            let _ = uart_write_all(uart, b"WIFISET WARN persist=timeout\r\n").await;
        }
    }
}
