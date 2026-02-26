use super::super::super::{
    config::WIFI_CREDENTIALS_UPDATES,
    runtime::service_mode,
    telemetry,
    types::{WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX},
};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use embassy_futures::select::{select3, Either3};
use embassy_time::{with_timeout, Duration, Timer};
use esp_println::println;
use esp_radio::wifi::{
    event::{self, EventExt},
    AccessPointInfo, AuthMethod, ClientConfig, Config as WifiRuntimeConfig, ModeConfig, ScanConfig,
    ScanMethod, ScanTypeConfig, WifiController, WifiEvent,
};
const WIFI_SCAN_DIAG_MAX_APS: usize = 64;
const WIFI_SCAN_ACTIVE_MIN_MS: u64 = 200;
const WIFI_SCAN_ACTIVE_MAX_MS: u64 = 600;
const WIFI_SCAN_PASSIVE_MS: u64 = 800;
const WIFI_CHANNEL_PROBE_SEQUENCE: [u8; 13] = [8, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13];
const WIFI_AUTH_METHODS: [AuthMethod; 5] = [
    AuthMethod::Wpa2Personal,
    AuthMethod::WpaWpa2Personal,
    AuthMethod::Wpa2Wpa3Personal,
    AuthMethod::Wpa3Personal,
    AuthMethod::Wpa,
];
const WIFI_REASON_BEACON_TIMEOUT: u8 = 200;
const WIFI_REASON_NO_AP_FOUND: u8 = 201;
const WIFI_REASON_AUTH_FAIL: u8 = 202;
const WIFI_REASON_ASSOC_FAIL: u8 = 203;
const WIFI_REASON_HANDSHAKE_TIMEOUT: u8 = 204;
const WIFI_REASON_CONNECTION_FAIL: u8 = 205;
const WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY: u8 = 210;
const WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD: u8 = 211;
const WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD: u8 = 212;
const WIFI_CONNECTED_WATCHDOG_MS: u64 = 2_000;
static WIFI_EVENT_LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static WIFI_LAST_DISCONNECT_REASON: AtomicU8 = AtomicU8::new(0);
pub(super) fn compiled_wifi_credentials() -> Option<WifiCredentials> {
    wifi_credentials().and_then(|(ssid, password)| {
        wifi_credentials_from_parts(ssid.as_bytes(), password.as_bytes()).ok()
    })
}
pub(super) fn wifi_runtime_config() -> WifiRuntimeConfig {
    WifiRuntimeConfig::default()
}
pub(super) async fn run_wifi_connection_task(
    mut controller: WifiController<'static>,
    mut credentials: Option<WifiCredentials>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let mut config_applied = false;
    let mut auth_method_idx = 0usize;
    let mut paused = false;
    let mut channel_hint = None;
    let mut channel_probe_idx = 0usize;
    if credentials.is_none() {
        println!("upload_http: waiting for WIFISET credentials over UART");
    }
    loop {
        if !service_mode::upload_enabled() {
            if !paused {
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                telemetry::set_wifi_link_connected(false);
                telemetry::set_upload_http_listener(false, None);
                paused = true;
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                channel_probe_idx = 0;
                println!("upload_http: upload mode off; wifi paused");
            }
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if paused {
            paused = false;
            println!("upload_http: upload mode on; wifi resuming");
        }

        while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
            if credentials == Some(updated) {
                println!("upload_http: wifi credentials unchanged; skipping reconfigure");
                continue;
            }
            credentials = Some(updated);
            config_applied = false;
            auth_method_idx = 0;
            channel_hint = None;
            channel_probe_idx = 0;
            println!("upload_http: wifi credentials updated");
        }

        let active = match credentials {
            Some(value) => value,
            None => {
                if let Ok(first) =
                    with_timeout(Duration::from_secs(3), WIFI_CREDENTIALS_UPDATES.receive()).await
                {
                    credentials = Some(first);
                    config_applied = false;
                    auth_method_idx = 0;
                    channel_hint = None;
                    channel_probe_idx = 0;
                    println!("upload_http: wifi credentials received");
                }
                continue;
            }
        };

        if !config_applied {
            let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
            let mode = match mode_config_from_credentials(active, auth_method, channel_hint) {
                Some(mode) => mode,
                None => {
                    println!("upload_http: wifi credentials invalid utf8 or length");
                    credentials = None;
                    continue;
                }
            };

            if let Err(err) = controller.set_config(&mode) {
                println!("upload_http: wifi station config err={:?}", err);
                if matches!(controller.is_started(), Ok(true)) {
                    let _ = controller.stop_async().await;
                }
                config_applied = false;
                Timer::after(Duration::from_secs(2)).await;
                continue;
            }
            println!(
                "upload_http: applying station config auth={:?} channel_hint={:?}",
                auth_method, channel_hint
            );
            config_applied = true;
        }

        match controller.is_started() {
            Ok(true) => {}
            Ok(false) => {
                if let Err(err) = controller.start_async().await {
                    println!("upload_http: wifi start err={:?}", err);
                    Timer::after(Duration::from_secs(3)).await;
                    continue;
                }
                Timer::after(Duration::from_millis(800)).await;
            }
            Err(err) => {
                println!("upload_http: wifi status err={:?}", err);
                Timer::after(Duration::from_secs(3)).await;
                continue;
            }
        }

        if channel_hint.is_none() {
            if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize]) {
                if let Some(channel) = log_scan_for_target(&mut controller, ssid).await {
                    channel_hint = Some(channel);
                    auth_method_idx = 0;
                    config_applied = false;
                    channel_probe_idx = 0;
                    println!(
                        "upload_http: pre-connect discovered channel_hint={}",
                        channel
                    );
                    Timer::after(Duration::from_millis(500)).await;
                    continue;
                }
            }
        }

        telemetry::record_wifi_connect_attempt(channel_hint, auth_method_idx);
        match controller.connect_async().await {
            Ok(()) => {
                telemetry::record_wifi_connect_success();
                println!("upload_http: wifi connected");
                loop {
                    match select3(
                        controller.wait_for_event(WifiEvent::StaDisconnected),
                        WIFI_CREDENTIALS_UPDATES.receive(),
                        Timer::after(Duration::from_millis(WIFI_CONNECTED_WATCHDOG_MS)),
                    )
                    .await
                    {
                        Either3::First(_) => {
                            telemetry::set_wifi_link_connected(false);
                            telemetry::set_upload_http_listener(false, None);
                            println!("upload_http: wifi disconnected");
                            break;
                        }
                        Either3::Second(updated) => {
                            if credentials == Some(updated) {
                                println!("upload_http: wifi credentials unchanged while connected");
                                continue;
                            }
                            credentials = Some(updated);
                            config_applied = false;
                            auth_method_idx = 0;
                            println!("upload_http: wifi credentials changed, reconnecting");
                            let _ = controller.disconnect_async().await;
                            telemetry::set_wifi_link_connected(false);
                            telemetry::set_upload_http_listener(false, None);
                            break;
                        }
                        Either3::Third(_) => {
                            if !service_mode::upload_enabled() {
                                telemetry::set_wifi_link_connected(false);
                                telemetry::set_upload_http_listener(false, None);
                                let _ = controller.disconnect_async().await;
                                println!("upload_http: upload mode off while connected");
                                break;
                            }
                            match controller.is_connected() {
                                Ok(true) => {}
                                Ok(false) | Err(_) => {
                                    telemetry::record_wifi_watchdog_disconnect();
                                    telemetry::set_wifi_link_connected(false);
                                    telemetry::set_upload_http_listener(false, None);
                                    let _ = controller.disconnect_async().await;
                                    println!(
                                        "upload_http: watchdog forced reconnect (is_connected=false)"
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(err) => {
                let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.swap(0, Ordering::Relaxed);
                telemetry::record_wifi_connect_failure(disconnect_reason);
                telemetry::set_upload_http_listener(false, None);
                let discovery_reason = is_discovery_disconnect_reason(disconnect_reason);
                let should_scan = channel_hint.is_none() || channel_probe_idx % 4 == 0;
                let mut observed_channel = None;
                if should_scan {
                    if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize])
                    {
                        observed_channel = log_scan_for_target(&mut controller, ssid).await;
                    }
                }
                let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
                println!(
                    "upload_http: wifi connect err={:?} auth={:?} channel_hint={:?} observed_channel={:?} reason={} (0x{:02x} {}) discovery_reason={} should_scan={} probe_idx={}",
                    err,
                    auth_method,
                    channel_hint,
                    observed_channel,
                    disconnect_reason,
                    disconnect_reason,
                    disconnect_reason_label(disconnect_reason),
                    discovery_reason,
                    should_scan,
                    channel_probe_idx,
                );
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                if let Some(channel) = observed_channel {
                    if channel_hint != Some(channel) {
                        channel_hint = Some(channel);
                        auth_method_idx = 0;
                        config_applied = false;
                        println!("upload_http: retrying with channel_hint={}", channel);
                        Timer::after(Duration::from_secs(2)).await;
                        continue;
                    }
                    println!(
                        "upload_http: keeping discovered channel_hint={} for next auth attempt",
                        channel
                    );
                }
                if channel_probe_idx < WIFI_CHANNEL_PROBE_SEQUENCE.len() {
                    let next_channel = next_probe_channel(&mut channel_probe_idx);
                    channel_hint = Some(next_channel);
                    auth_method_idx = 0;
                    config_applied = false;
                    println!(
                        "upload_http: channel probe retry using channel_hint={} probe_idx={}",
                        next_channel, channel_probe_idx
                    );
                    Timer::after(Duration::from_secs(2)).await;
                    continue;
                }
                channel_probe_idx = 0;
                channel_hint = None;
                auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                config_applied = false;
                println!(
                    "upload_http: rotating auth after channel sweep auth={:?}",
                    WIFI_AUTH_METHODS[auth_method_idx]
                );
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }
}

fn install_wifi_event_logger() {
    if !cfg!(debug_assertions) {
        return;
    }
    if WIFI_EVENT_LOGGER_INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    event::StaStart::update_handler(|_| {
        println!("upload_http: event sta_start");
    });

    event::StaStop::update_handler(|_| {
        println!("upload_http: event sta_stop");
    });

    event::ScanDone::update_handler(|event| {
        println!(
            "upload_http: event scan_done status={} count={} scan_id={}",
            event.status(),
            event.number(),
            event.id()
        );
    });

    event::StaConnected::update_handler(|event| {
        let ssid_len = (event.ssid_len() as usize).min(event.ssid().len());
        let ssid = core::str::from_utf8(&event.ssid()[..ssid_len]).unwrap_or("<non_utf8>");
        println!(
            "upload_http: event sta_connected ssid={} channel={} authmode={}",
            ssid,
            event.channel(),
            event.authmode()
        );
    });

    event::StaDisconnected::update_handler(|event| {
        let reason = event.reason();
        WIFI_LAST_DISCONNECT_REASON.store(reason, Ordering::Relaxed);
        println!(
            "upload_http: event sta_disconnected reason={} ({}) rssi={}",
            reason,
            disconnect_reason_label(reason),
            event.rssi()
        );
    });
}

fn disconnect_reason_label(reason: u8) -> &'static str {
    match reason {
        WIFI_REASON_BEACON_TIMEOUT => "beacon_timeout",
        WIFI_REASON_NO_AP_FOUND => "no_ap_found",
        WIFI_REASON_AUTH_FAIL => "auth_fail",
        WIFI_REASON_ASSOC_FAIL => "assoc_fail",
        WIFI_REASON_HANDSHAKE_TIMEOUT => "handshake_timeout",
        WIFI_REASON_CONNECTION_FAIL => "connection_fail",
        WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY => "no_ap_found_compatible_security",
        WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD => "no_ap_found_authmode_threshold",
        WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD => "no_ap_found_rssi_threshold",
        _ => "other",
    }
}

fn is_discovery_disconnect_reason(reason: u8) -> bool {
    reason == WIFI_REASON_BEACON_TIMEOUT
        || reason == WIFI_REASON_NO_AP_FOUND
        || reason == WIFI_REASON_NO_AP_FOUND_COMPAT_SECURITY
        || reason == WIFI_REASON_NO_AP_FOUND_AUTHMODE_THRESHOLD
        || reason == WIFI_REASON_NO_AP_FOUND_RSSI_THRESHOLD
}

fn next_probe_channel(index: &mut usize) -> u8 {
    let channel = WIFI_CHANNEL_PROBE_SEQUENCE[*index % WIFI_CHANNEL_PROBE_SEQUENCE.len()];
    *index = index.saturating_add(1);
    channel
}

fn wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("MEDITAMER_WIFI_SSID").or(option_env!("SSID"))?;
    let password = option_env!("MEDITAMER_WIFI_PASSWORD")
        .or(option_env!("PASSWORD"))
        .unwrap_or("");
    Some((ssid, password))
}

fn wifi_credentials_from_parts(
    ssid: &[u8],
    password: &[u8],
) -> Result<WifiCredentials, &'static str> {
    if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX || password.len() > WIFI_PASSWORD_MAX {
        return Err("invalid wifi credentials length");
    }
    let mut result = WifiCredentials {
        ssid: [0u8; WIFI_SSID_MAX],
        ssid_len: ssid.len() as u8,
        password: [0u8; WIFI_PASSWORD_MAX],
        password_len: password.len() as u8,
    };
    result.ssid[..ssid.len()].copy_from_slice(ssid);
    result.password[..password.len()].copy_from_slice(password);
    Ok(result)
}

fn mode_config_from_credentials(
    credentials: WifiCredentials,
    auth_method: AuthMethod,
    channel_hint: Option<u8>,
) -> Option<ModeConfig> {
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).ok()?;
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).ok()?;
    let auth_method = if password.is_empty() {
        AuthMethod::None
    } else {
        auth_method
    };
    let scan_method = if channel_hint.is_some() {
        ScanMethod::Fast
    } else {
        ScanMethod::AllChannels
    };
    let mut client = ClientConfig::default()
        .with_ssid(ssid.into())
        .with_password(password.into())
        .with_auth_method(auth_method)
        .with_scan_method(scan_method);
    if let Some(channel) = channel_hint {
        client = client.with_channel(channel);
    }
    Some(ModeConfig::Client(client))
}

async fn log_scan_for_target(
    controller: &mut WifiController<'static>,
    target_ssid: &str,
) -> Option<u8> {
    let mut discovered_channel = None;

    let active = ScanConfig::default()
        .with_show_hidden(true)
        .with_max(WIFI_SCAN_DIAG_MAX_APS)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(WIFI_SCAN_ACTIVE_MIN_MS).into(),
            max: Duration::from_millis(WIFI_SCAN_ACTIVE_MAX_MS).into(),
        });
    match controller.scan_with_config_async(active).await {
        Ok(results) => {
            discovered_channel = log_scan_results("active", target_ssid, &results);
        }
        Err(err) => {
            println!(
                "upload_http: scan active err={:?} target_ssid={}",
                err, target_ssid
            );
        }
    }
    if discovered_channel.is_some() {
        return discovered_channel;
    }

    let passive = ScanConfig::default()
        .with_show_hidden(true)
        .with_max(WIFI_SCAN_DIAG_MAX_APS)
        .with_scan_type(ScanTypeConfig::Passive(
            Duration::from_millis(WIFI_SCAN_PASSIVE_MS).into(),
        ));
    match controller.scan_with_config_async(passive).await {
        Ok(results) => {
            discovered_channel = log_scan_results("passive", target_ssid, &results);
        }
        Err(err) => {
            println!(
                "upload_http: scan passive err={:?} target_ssid={}",
                err, target_ssid
            );
        }
    }

    if discovered_channel.is_some() {
        println!(
            "upload_http: scan target_ssid={} found_channel={:?}",
            target_ssid, discovered_channel
        );
        return discovered_channel;
    }

    println!("upload_http: scan target_ssid={} found=0", target_ssid);
    None
}

fn log_scan_results(label: &str, target_ssid: &str, results: &[AccessPointInfo]) -> Option<u8> {
    if results.is_empty() {
        telemetry::record_wifi_scan(0, false);
        println!(
            "upload_http: scan {} found=0 target_ssid={}",
            label, target_ssid
        );
        return None;
    }

    println!(
        "upload_http: scan {} found={} target_ssid={}",
        label,
        results.len(),
        target_ssid
    );

    let mut discovered_channel = None;
    for ap in results.iter() {
        println!(
            "upload_http: scan ap ssid={} channel={} rssi={} auth={:?}",
            ap.ssid, ap.channel, ap.signal_strength, ap.auth_method
        );
        if discovered_channel.is_none() && ap.ssid == target_ssid {
            discovered_channel = Some(ap.channel);
        }
    }

    if let Some(channel) = discovered_channel {
        println!(
            "upload_http: scan target_ssid={} found_channel={} via={}",
            target_ssid, channel, label
        );
    }
    telemetry::record_wifi_scan(results.len(), discovered_channel.is_some());
    discovered_channel
}
