use core::{
    fmt::Write,
    sync::atomic::{AtomicU32, Ordering},
};

use embassy_time::{with_timeout, Duration};

use crate::firmware::{
    config::{
        NET_CONFIG_SET_UPDATES, WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES,
        WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
    },
    net::wifi,
    touch::debug_log::uart_write_all,
    types::{NetConfigSet, WifiConfigRequest, WifiConfigResultCode},
};

use super::netcfg_persistence::{classify_persistence_response, PersistenceResponseDisposition};
use super::SerialUart;

static NEXT_WIFI_CONFIG_REQUEST_ID: AtomicU32 = AtomicU32::new(1);

fn next_wifi_config_request_id() -> u32 {
    loop {
        let request_id = NEXT_WIFI_CONFIG_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        if request_id != 0 {
            return request_id;
        }
    }
}

pub(crate) async fn run_netcfg_set_command(uart: &mut SerialUart, config: NetConfigSet) {
    while NET_CONFIG_SET_UPDATES.try_receive().is_ok() {}
    while WIFI_CONFIG_RESPONSES.try_receive().is_ok() {}

    if NET_CONFIG_SET_UPDATES.try_send(config).is_err() {
        let _ = uart_write_all(uart, b"NET ERR reason=busy\r\n").await;
        return;
    }
    wifi::remember_runtime_config(config);

    if let Some(credentials) = config.credentials {
        let request_id = next_wifi_config_request_id();
        WIFI_CONFIG_REQUESTS
            .send(WifiConfigRequest::Store {
                request_id,
                credentials,
            })
            .await;
        let persist_result = with_timeout(
            Duration::from_millis(WIFI_CONFIG_RESPONSE_TIMEOUT_MS),
            async {
                loop {
                    let response = WIFI_CONFIG_RESPONSES.receive().await;
                    match classify_persistence_response(
                        request_id,
                        response.request_id,
                        response.ok,
                        response.code == WifiConfigResultCode::Ok,
                    ) {
                        PersistenceResponseDisposition::Stale => continue,
                        PersistenceResponseDisposition::Succeeded => return Ok(()),
                        PersistenceResponseDisposition::Failed => return Err(response.code),
                    }
                }
            },
        )
        .await;
        match persist_result {
            Ok(Ok(())) => {}
            Ok(Err(code)) => {
                let mut line = heapless::String::<96>::new();
                let _ = write!(
                    &mut line,
                    "NET ERR reason=persist_failed code={}\r\n",
                    wifi_config_result_code_label(code)
                );
                let _ = uart_write_all(uart, line.as_bytes()).await;
                return;
            }
            Err(_) => {
                let _ = uart_write_all(uart, b"NET ERR reason=persist_timeout\r\n").await;
                return;
            }
        }
    }

    let _ = uart_write_all(uart, b"NET OK op=config_set\r\n").await;
}

pub(crate) async fn run_netcfg_get_command(uart: &mut SerialUart) {
    let snapshot = wifi::net_config_snapshot();
    let mut line = heapless::String::<512>::new();
    let _ = write!(
        &mut line,
        "NETCFG {{\"ssid_set\":{},\"ssid\":\"{}\",\"policy\":{{\"connect_timeout_ms\":{},\"dhcp_timeout_ms\":{},\"pinned_dhcp_timeout_ms\":{},\"listener_timeout_ms\":{},\"scan_active_min_ms\":{},\"scan_active_max_ms\":{},\"scan_passive_ms\":{},\"retry_same_max\":{},\"rotate_candidate_max\":{},\"rotate_auth_max\":{},\"full_scan_reset_max\":{},\"driver_restart_max\":{},\"cooldown_ms\":{},\"driver_restart_backoff_ms\":{}}}}}\r\n",
        if snapshot.credentials_set { "true" } else { "false" },
        snapshot.ssid,
        snapshot.policy.connect_timeout_ms,
        snapshot.policy.dhcp_timeout_ms,
        snapshot.policy.pinned_dhcp_timeout_ms,
        snapshot.policy.listener_timeout_ms,
        snapshot.policy.scan_active_min_ms,
        snapshot.policy.scan_active_max_ms,
        snapshot.policy.scan_passive_ms,
        snapshot.policy.retry_same_max,
        snapshot.policy.rotate_candidate_max,
        snapshot.policy.rotate_auth_max,
        snapshot.policy.full_scan_reset_max,
        snapshot.policy.driver_restart_max,
        snapshot.policy.cooldown_ms,
        snapshot.policy.driver_restart_backoff_ms,
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

fn wifi_config_result_code_label(code: WifiConfigResultCode) -> &'static str {
    match code {
        WifiConfigResultCode::Ok => "ok",
        WifiConfigResultCode::Busy => "busy",
        WifiConfigResultCode::NotFound => "not_found",
        WifiConfigResultCode::InvalidData => "invalid_data",
        WifiConfigResultCode::PowerOnFailed => "power_on_failed",
        WifiConfigResultCode::InitFailed => "init_failed",
        WifiConfigResultCode::OperationFailed => "operation_failed",
    }
}
