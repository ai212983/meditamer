use super::super::super::{
    config::WIFI_CREDENTIALS_UPDATES,
    runtime::service_mode,
    telemetry,
    types::{WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX},
};
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use embassy_net::Stack;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_println::println;
use esp_radio::wifi::{
    event::{self, EventExt},
    AccessPointInfo, AuthMethod, ClientConfig, Config as WifiRuntimeConfig, ModeConfig, ScanConfig,
    ScanMethod, ScanTypeConfig, WifiController,
};
const WIFI_SCAN_DIAG_MAX_APS: usize = 64;
const WIFI_SCAN_ACTIVE_MIN_MS: u64 = 600;
const WIFI_SCAN_ACTIVE_MAX_MS: u64 = 1_500;
const WIFI_SCAN_PASSIVE_MS: u64 = 1_500;
const WIFI_CHANNEL_PROBE_SEQUENCE: [u8; 13] = [8, 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13];
const WIFI_AUTH_METHODS: [AuthMethod; 5] = [
    AuthMethod::WpaWpa2Personal,
    AuthMethod::Wpa2Personal,
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
const WIFI_DHCP_LEASE_TIMEOUT_MS: u64 = 45_000;
const WIFI_DHCP_LEASE_TIMEOUT_PINNED_BSSID_MS: u64 = 15_000;
static WIFI_EVENT_LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);
static WIFI_LAST_DISCONNECT_REASON: AtomicU8 = AtomicU8::new(0);
static WIFI_DISCONNECTED_EVENT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TargetApHint {
    channel: u8,
    bssid: [u8; 6],
}

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
    stack: Stack<'static>,
) {
    install_wifi_event_logger();
    telemetry::set_wifi_link_connected(false);
    let mut config_applied = false;
    let mut auth_method_idx = 0usize;
    let mut paused = false;
    let mut channel_hint = None;
    let mut bssid_hint = None;
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
                telemetry::record_wifi_reassoc_mode_pause();
                paused = true;
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                bssid_hint = None;
                channel_probe_idx = 0;
                println!("upload_http: upload mode off; wifi paused");
            }
            Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        if paused {
            paused = false;
            telemetry::record_wifi_reassoc_mode_resume();
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
            bssid_hint = None;
            channel_probe_idx = 0;
            telemetry::record_wifi_reassoc_credentials_changed();
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
                    bssid_hint = None;
                    channel_probe_idx = 0;
                    telemetry::record_wifi_reassoc_credentials_received();
                    println!("upload_http: wifi credentials received");
                }
                continue;
            }
        };

        if !config_applied {
            let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
            let mode =
                match mode_config_from_credentials(active, auth_method, channel_hint, bssid_hint) {
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
                "upload_http: applying station config auth={:?} channel_hint={:?} bssid_hint={}",
                auth_method,
                channel_hint,
                format_bssid_opt(bssid_hint),
            );
            telemetry::record_wifi_reassoc_config_applied(
                auth_method_idx,
                channel_hint,
                channel_probe_idx,
            );
            config_applied = true;
        }

        match controller.is_started() {
            Ok(true) => {}
            Ok(false) => {
                if let Err(err) = controller.start_async().await {
                    println!("upload_http: wifi start err={:?}", err);
                    telemetry::record_wifi_reassoc_start_err();
                    Timer::after(Duration::from_secs(3)).await;
                    continue;
                }
                telemetry::record_wifi_reassoc_start_ok();
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
                    channel_hint = Some(channel.channel);
                    bssid_hint = None;
                    auth_method_idx = 0;
                    config_applied = false;
                    channel_probe_idx = 0;
                    println!(
                        "upload_http: pre-connect discovered channel_hint={} (observed_bssid={})",
                        channel.channel,
                        format_bssid(channel.bssid),
                    );
                    Timer::after(Duration::from_millis(500)).await;
                    continue;
                }
            }
        }

        // Reset stale disconnect state before this connect attempt so we do not
        // miss a fresh disconnect that races right after connect succeeds.
        WIFI_DISCONNECTED_EVENT.store(false, Ordering::Relaxed);
        telemetry::record_wifi_reassoc_connect_begin(
            auth_method_idx,
            channel_hint,
            channel_probe_idx,
        );
        telemetry::record_wifi_connect_attempt(channel_hint, auth_method_idx);
        let connect_started_at = Instant::now();
        match controller.connect_async().await {
            Ok(()) => {
                telemetry::record_wifi_connect_success();
                telemetry::record_wifi_reassoc_connect_success(elapsed_ms_u32(connect_started_at));
                println!("upload_http: wifi connected");
                let mut dhcp_lease_observed = has_ipv4_lease(&stack);
                let dhcp_wait_started_at = Instant::now();
                loop {
                    if !service_mode::upload_enabled() {
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        let _ = controller.disconnect_async().await;
                        println!("upload_http: upload mode off while connected");
                        break;
                    }

                    let mut reconnect_due_to_credentials = false;
                    while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
                        if credentials == Some(updated) {
                            println!("upload_http: wifi credentials unchanged while connected");
                            continue;
                        }
                        credentials = Some(updated);
                        config_applied = false;
                        auth_method_idx = 0;
                        channel_hint = None;
                        bssid_hint = None;
                        channel_probe_idx = 0;
                        reconnect_due_to_credentials = true;
                    }
                    if reconnect_due_to_credentials {
                        println!("upload_http: wifi credentials changed, reconnecting");
                        telemetry::record_wifi_reassoc_credentials_changed();
                        let _ = controller.disconnect_async().await;
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        break;
                    }

                    if WIFI_DISCONNECTED_EVENT.swap(false, Ordering::Relaxed) {
                        telemetry::record_wifi_reassoc_disconnect_event(
                            WIFI_LAST_DISCONNECT_REASON.load(Ordering::Relaxed),
                        );
                        telemetry::set_wifi_link_connected(false);
                        telemetry::set_upload_http_listener(false, None);
                        println!("upload_http: wifi disconnected");
                        break;
                    }

                    if !dhcp_lease_observed {
                        dhcp_lease_observed = has_ipv4_lease(&stack);
                        if !dhcp_lease_observed {
                            let dhcp_timeout_ms = if bssid_hint.is_some() {
                                WIFI_DHCP_LEASE_TIMEOUT_PINNED_BSSID_MS
                            } else {
                                WIFI_DHCP_LEASE_TIMEOUT_MS
                            };
                            if dhcp_wait_started_at.elapsed().as_millis() >= dhcp_timeout_ms {
                                if bssid_hint.take().is_some() {
                                    println!(
                                        "upload_http: dhcp timeout on pinned bssid; clearing bssid hint and reconnecting"
                                    );
                                } else {
                                    println!(
                                        "upload_http: dhcp timeout; reconnecting and retrying scan/auth"
                                    );
                                    channel_hint = None;
                                    channel_probe_idx = 0;
                                }
                                telemetry::record_wifi_watchdog_disconnect();
                                let _ = controller.disconnect_async().await;
                                telemetry::set_wifi_link_connected(false);
                                telemetry::set_upload_http_listener(false, None);
                                break;
                            }
                        }
                    }

                    Timer::after(Duration::from_millis(WIFI_CONNECTED_WATCHDOG_MS)).await;
                }
            }
            Err(err) => {
                let disconnect_reason = WIFI_LAST_DISCONNECT_REASON.swap(0, Ordering::Relaxed);
                telemetry::record_wifi_connect_failure(disconnect_reason);
                telemetry::record_wifi_reassoc_connect_failure_detail(
                    disconnect_reason,
                    elapsed_ms_u32(connect_started_at),
                );
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
                    "upload_http: wifi connect err={:?} auth={:?} channel_hint={:?} bssid_hint={} observed_channel={:?} observed_bssid={} reason={} (0x{:02x} {}) discovery_reason={} should_scan={} probe_idx={}",
                    err,
                    auth_method,
                    channel_hint,
                    format_bssid_opt(bssid_hint),
                    observed_channel,
                    format_bssid_opt(observed_channel.map(|ap| ap.bssid)),
                    disconnect_reason,
                    disconnect_reason,
                    disconnect_reason_label(disconnect_reason),
                    discovery_reason,
                    should_scan,
                    channel_probe_idx,
                );
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                if let Some(ap) = observed_channel {
                    if channel_hint != Some(ap.channel)
                        || (bssid_hint.is_some() && bssid_hint != Some(ap.bssid))
                    {
                        channel_hint = Some(ap.channel);
                        bssid_hint = None;
                        auth_method_idx = 0;
                        config_applied = false;
                        telemetry::record_wifi_reassoc_hint_retry(
                            ap.channel,
                            auth_method_idx,
                            channel_probe_idx,
                        );
                        println!(
                            "upload_http: retrying with channel_hint={} (observed_bssid={})",
                            ap.channel,
                            format_bssid(ap.bssid),
                        );
                        Timer::after(Duration::from_secs(2)).await;
                        continue;
                    }
                    println!(
                        "upload_http: keeping discovered channel_hint={} bssid_hint={} for next auth attempt",
                        ap.channel,
                        format_bssid(ap.bssid),
                    );
                }
                if channel_hint.is_some() {
                    auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                    config_applied = false;
                    telemetry::record_wifi_reassoc_auth_rotation(
                        auth_method_idx,
                        channel_hint,
                        channel_probe_idx,
                    );
                    println!(
                        "upload_http: rotating auth on hinted channel auth={:?} channel_hint={:?} bssid_hint={}",
                        WIFI_AUTH_METHODS[auth_method_idx],
                        channel_hint,
                        format_bssid_opt(bssid_hint),
                    );
                    Timer::after(Duration::from_secs(2)).await;
                    continue;
                }
                if channel_probe_idx < WIFI_CHANNEL_PROBE_SEQUENCE.len() {
                    let next_channel = next_probe_channel(&mut channel_probe_idx);
                    channel_hint = Some(next_channel);
                    bssid_hint = None;
                    auth_method_idx = 0;
                    config_applied = false;
                    telemetry::record_wifi_reassoc_channel_probe(next_channel, channel_probe_idx);
                    println!(
                        "upload_http: channel probe retry using channel_hint={} probe_idx={}",
                        next_channel, channel_probe_idx
                    );
                    Timer::after(Duration::from_secs(2)).await;
                    continue;
                }
                channel_probe_idx = 0;
                channel_hint = None;
                bssid_hint = None;
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
    if WIFI_EVENT_LOGGER_INSTALLED.swap(true, Ordering::Relaxed) {
        return;
    }

    event::StaDisconnected::update_handler(|event| {
        let reason = event.reason();
        WIFI_LAST_DISCONNECT_REASON.store(reason, Ordering::Relaxed);
        WIFI_DISCONNECTED_EVENT.store(true, Ordering::Relaxed);
        if cfg!(debug_assertions) {
            println!(
                "upload_http: event sta_disconnected reason={} ({}) rssi={}",
                reason,
                disconnect_reason_label(reason),
                event.rssi()
            );
        }
    });

    if !cfg!(debug_assertions) {
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
        let bssid = event.bssid();
        println!(
            "upload_http: event sta_connected ssid={} channel={} authmode={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            ssid,
            event.channel(),
            event.authmode(),
            bssid.get(0).copied().unwrap_or(0),
            bssid.get(1).copied().unwrap_or(0),
            bssid.get(2).copied().unwrap_or(0),
            bssid.get(3).copied().unwrap_or(0),
            bssid.get(4).copied().unwrap_or(0),
            bssid.get(5).copied().unwrap_or(0),
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
    bssid_hint: Option<[u8; 6]>,
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
    if let Some(bssid) = bssid_hint {
        client = client.with_bssid(bssid);
    }
    Some(ModeConfig::Client(client))
}

async fn log_scan_for_target(
    controller: &mut WifiController<'static>,
    target_ssid: &str,
) -> Option<TargetApHint> {
    let mut discovered_channel = None;

    let active = ScanConfig::default()
        .with_ssid(target_ssid)
        .with_show_hidden(true)
        .with_max(WIFI_SCAN_DIAG_MAX_APS)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(WIFI_SCAN_ACTIVE_MIN_MS).into(),
            max: Duration::from_millis(WIFI_SCAN_ACTIVE_MAX_MS).into(),
        });
    let active_started_at = Instant::now();
    match controller.scan_with_config_async(active).await {
        Ok(results) => {
            discovered_channel = log_scan_results("active", target_ssid, &results);
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                results.len(),
                discovered_channel.is_some(),
                elapsed_ms_u32(active_started_at),
                discovered_channel.map(|ap| ap.channel),
            );
        }
        Err(err) => {
            println!(
                "upload_http: scan active err={:?} target_ssid={}",
                err, target_ssid
            );
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Active,
                0,
                false,
                elapsed_ms_u32(active_started_at),
                None,
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
    let passive_started_at = Instant::now();
    match controller.scan_with_config_async(passive).await {
        Ok(results) => {
            discovered_channel = log_scan_results("passive", target_ssid, &results);
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Passive,
                results.len(),
                discovered_channel.is_some(),
                elapsed_ms_u32(passive_started_at),
                discovered_channel.map(|ap| ap.channel),
            );
        }
        Err(err) => {
            println!(
                "upload_http: scan passive err={:?} target_ssid={}",
                err, target_ssid
            );
            telemetry::record_wifi_reassoc_scan(
                telemetry::WifiScanPhase::Passive,
                0,
                false,
                elapsed_ms_u32(passive_started_at),
                None,
            );
        }
    }

    if discovered_channel.is_some() {
        println!(
            "upload_http: scan target_ssid={} found_channel={:?} found_bssid={}",
            target_ssid,
            discovered_channel.map(|ap| ap.channel),
            format_bssid_opt(discovered_channel.map(|ap| ap.bssid)),
        );
        return discovered_channel;
    }

    println!("upload_http: scan target_ssid={} found=0", target_ssid);
    None
}

fn log_scan_results(
    label: &str,
    target_ssid: &str,
    results: &[AccessPointInfo],
) -> Option<TargetApHint> {
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

    let mut discovered_ap: Option<&AccessPointInfo> = None;
    for ap in results.iter() {
        println!(
            "upload_http: scan ap ssid={} channel={} bssid={} rssi={} auth={:?}",
            ap.ssid,
            ap.channel,
            format_bssid(ap.bssid),
            ap.signal_strength,
            ap.auth_method
        );
        if ap.ssid == target_ssid {
            let replace = match discovered_ap {
                Some(best) => ap.signal_strength > best.signal_strength,
                None => true,
            };
            if replace {
                discovered_ap = Some(ap);
            }
        }
    }

    let discovered_channel = discovered_ap.map(|ap| TargetApHint {
        channel: ap.channel,
        bssid: ap.bssid,
    });

    if let Some(ap) = discovered_channel {
        println!(
            "upload_http: scan target_ssid={} found_channel={} found_bssid={} via={}",
            target_ssid,
            ap.channel,
            format_bssid(ap.bssid),
            label
        );
    }
    telemetry::record_wifi_scan(results.len(), discovered_channel.is_some());
    discovered_channel
}

fn format_bssid(bssid: [u8; 6]) -> heapless::String<17> {
    let mut out = heapless::String::<17>::new();
    let _ = write!(
        out,
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        bssid[0], bssid[1], bssid[2], bssid[3], bssid[4], bssid[5]
    );
    out
}

fn format_bssid_opt(bssid: Option<[u8; 6]>) -> heapless::String<17> {
    match bssid {
        Some(value) => format_bssid(value),
        None => {
            let mut out = heapless::String::<17>::new();
            let _ = out.push_str("<none>");
            out
        }
    }
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn has_ipv4_lease(stack: &Stack<'static>) -> bool {
    stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
        .is_some()
}
