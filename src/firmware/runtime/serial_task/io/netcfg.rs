use core::fmt::Write;

use embassy_time::{with_timeout, Duration};

use crate::firmware::{
    config::{
        NET_CONFIG_SET_UPDATES, WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES,
        WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
    },
    storage::upload::wifi,
    touch::debug_log::uart_write_all,
    types::{NetConfigSet, WifiConfigRequest},
};

use super::SerialUart;

pub(crate) async fn run_netcfg_set_command(uart: &mut SerialUart, config: NetConfigSet) {
    while NET_CONFIG_SET_UPDATES.try_receive().is_ok() {}
    while WIFI_CONFIG_RESPONSES.try_receive().is_ok() {}

    if NET_CONFIG_SET_UPDATES.try_send(config).is_err() {
        let _ = uart_write_all(uart, b"NET ERR reason=busy\r\n").await;
        return;
    }

    if let Some(credentials) = config.credentials {
        WIFI_CONFIG_REQUESTS
            .send(WifiConfigRequest::Store { credentials })
            .await;
        let persist_result = with_timeout(
            Duration::from_millis(WIFI_CONFIG_RESPONSE_TIMEOUT_MS),
            WIFI_CONFIG_RESPONSES.receive(),
        )
        .await;
        if persist_result.is_err() {
            let _ = uart_write_all(uart, b"NET ERR reason=persist_timeout\r\n").await;
            return;
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
