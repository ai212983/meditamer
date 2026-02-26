use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Timer};
use esp_println::println;
use esp_radio::wifi::{
    event::{self, EventExt},
    AuthMethod, ClientConfig, Config as WifiRuntimeConfig, ModeConfig, ScanConfig, ScanMethod,
    ScanTypeConfig, WifiController, WifiEvent,
};

use super::super::super::{
    config::{
        WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES, WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
        WIFI_CREDENTIALS_UPDATES,
    },
    runtime::service_mode,
    types::{
        WifiConfigRequest, WifiConfigResultCode, WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
    },
};

const WIFI_RX_QUEUE_SIZE: usize = 3;
const WIFI_TX_QUEUE_SIZE: usize = 2;
const WIFI_STATIC_RX_BUF_NUM: u8 = 4;
const WIFI_DYNAMIC_RX_BUF_NUM: u16 = 8;
const WIFI_DYNAMIC_TX_BUF_NUM: u16 = 8;
const WIFI_RX_BA_WIN: u8 = 3;
const WIFI_LOG_SCAN_ON_CONNECT: bool = cfg!(debug_assertions);
const WIFI_SCAN_DIAG_MAX_APS: usize = 16;
const WIFI_SCAN_ACTIVE_MIN_MS: u64 = 80;
const WIFI_SCAN_ACTIVE_MAX_MS: u64 = 240;
const WIFI_TARGET_CHANNEL_PROBE: Option<u8> = Some(8);
const WIFI_AUTH_METHODS: [AuthMethod; 4] = [
    AuthMethod::Wpa2Personal,
    AuthMethod::WpaWpa2Personal,
    AuthMethod::Wpa2Wpa3Personal,
    AuthMethod::Wpa,
];
static WIFI_EVENT_LOGGER_INSTALLED: AtomicBool = AtomicBool::new(false);

pub(super) fn compiled_wifi_credentials() -> Option<WifiCredentials> {
    wifi_credentials().and_then(|(ssid, password)| {
        wifi_credentials_from_parts(ssid.as_bytes(), password.as_bytes()).ok()
    })
}

pub(super) fn wifi_runtime_config() -> WifiRuntimeConfig {
    WifiRuntimeConfig::default()
        .with_rx_queue_size(WIFI_RX_QUEUE_SIZE)
        .with_tx_queue_size(WIFI_TX_QUEUE_SIZE)
        .with_static_rx_buf_num(WIFI_STATIC_RX_BUF_NUM)
        .with_dynamic_rx_buf_num(WIFI_DYNAMIC_RX_BUF_NUM)
        .with_dynamic_tx_buf_num(WIFI_DYNAMIC_TX_BUF_NUM)
        .with_ampdu_rx_enable(false)
        .with_ampdu_tx_enable(false)
        .with_rx_ba_win(WIFI_RX_BA_WIN)
}

pub(super) async fn run_wifi_connection_task(
    mut controller: WifiController<'static>,
    mut credentials: Option<WifiCredentials>,
) {
    install_wifi_event_logger();

    let mut config_applied = false;
    let mut auth_method_idx = 0usize;
    let mut paused = false;
    let mut channel_hint = None;

    if let Some(sd_credentials) = load_wifi_credentials_from_sd().await {
        credentials = Some(sd_credentials);
        config_applied = false;
        auth_method_idx = 0;
        channel_hint = None;
        println!("upload_http: loaded wifi credentials from SD");
    }

    if credentials.is_none() {
        println!("upload_http: waiting for WIFISET credentials over UART");
    }

    loop {
        if !service_mode::upload_enabled() {
            if !paused {
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                paused = true;
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
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
            credentials = Some(updated);
            config_applied = false;
            auth_method_idx = 0;
            channel_hint = None;
            println!("upload_http: wifi credentials updated");
        }

        let active = match credentials {
            Some(value) => value,
            None => {
                let first = WIFI_CREDENTIALS_UPDATES.receive().await;
                credentials = Some(first);
                config_applied = false;
                auth_method_idx = 0;
                channel_hint = None;
                println!("upload_http: wifi credentials received");
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

        let mut observed_channel = None;
        if WIFI_LOG_SCAN_ON_CONNECT {
            if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize]) {
                observed_channel = log_scan_for_target(&mut controller, ssid).await;
            }
        }

        match controller.connect_async().await {
            Ok(()) => {
                println!("upload_http: wifi connected");
                match select(
                    controller.wait_for_event(WifiEvent::StaDisconnected),
                    WIFI_CREDENTIALS_UPDATES.receive(),
                )
                .await
                {
                    Either::First(_) => {
                        println!("upload_http: wifi disconnected");
                    }
                    Either::Second(updated) => {
                        credentials = Some(updated);
                        config_applied = false;
                        auth_method_idx = 0;
                        println!("upload_http: wifi credentials changed, reconnecting");
                        let _ = controller.disconnect_async().await;
                    }
                }
            }
            Err(err) => {
                let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
                println!(
                    "upload_http: wifi connect err={:?} auth={:?} channel_hint={:?} observed_channel={:?}",
                    err, auth_method, channel_hint, observed_channel
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
                }
                auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                config_applied = false;
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
        200 => "beacon_timeout",
        201 => "no_ap_found",
        202 => "auth_fail",
        203 => "assoc_fail",
        204 => "handshake_timeout",
        205 => "connection_fail",
        210 => "no_ap_found_compatible_security",
        211 => "no_ap_found_authmode_threshold",
        212 => "no_ap_found_rssi_threshold",
        _ => "other",
    }
}

fn wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("MEDITAMER_WIFI_SSID").or(option_env!("SSID"))?;
    let password = option_env!("MEDITAMER_WIFI_PASSWORD")
        .or(option_env!("PASSWORD"))
        .unwrap_or("");
    Some((ssid, password))
}

async fn load_wifi_credentials_from_sd() -> Option<WifiCredentials> {
    drain_wifi_config_responses();
    WIFI_CONFIG_REQUESTS.send(WifiConfigRequest::Load).await;
    let response = with_timeout(
        Duration::from_millis(WIFI_CONFIG_RESPONSE_TIMEOUT_MS),
        WIFI_CONFIG_RESPONSES.receive(),
    )
    .await
    .ok()?;

    if response.ok {
        return response.credentials;
    }

    match response.code {
        WifiConfigResultCode::NotFound => {}
        WifiConfigResultCode::InvalidData => {
            println!("upload_http: SD wifi config invalid; waiting for WIFISET")
        }
        code => println!("upload_http: SD wifi config load failed code={:?}", code),
    }
    None
}

fn drain_wifi_config_responses() {
    while WIFI_CONFIG_RESPONSES.try_receive().is_ok() {}
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
    let mut client = ClientConfig::default()
        .with_ssid(ssid.into())
        .with_password(password.into())
        .with_auth_method(auth_method)
        .with_scan_method(ScanMethod::AllChannels);
    if let Some(channel) = channel_hint {
        client = client.with_channel(channel);
    }
    Some(ModeConfig::Client(client))
}

async fn log_scan_for_target(
    controller: &mut WifiController<'static>,
    target_ssid: &str,
) -> Option<u8> {
    let config = ScanConfig::default()
        .with_show_hidden(false)
        .with_max(WIFI_SCAN_DIAG_MAX_APS)
        .with_scan_type(ScanTypeConfig::Active {
            min: Duration::from_millis(WIFI_SCAN_ACTIVE_MIN_MS).into(),
            max: Duration::from_millis(WIFI_SCAN_ACTIVE_MAX_MS).into(),
        });

    let mut discovered_channel = None;
    match controller.scan_with_config_async(config).await {
        Ok(results) if results.is_empty() => {
            println!("upload_http: scan all found=0 target_ssid={}", target_ssid);
        }
        Ok(results) => {
            println!(
                "upload_http: scan all found={} target_ssid={}",
                results.len(),
                target_ssid,
            );
            for ap in results.iter() {
                println!(
                    "upload_http: scan ap ssid={} channel={} rssi={} auth={:?}",
                    ap.ssid, ap.channel, ap.signal_strength, ap.auth_method
                );
                if discovered_channel.is_none() && ap.ssid == target_ssid {
                    discovered_channel = Some(ap.channel);
                }
            }
        }
        Err(err) => {
            println!(
                "upload_http: scan all err={:?} target_ssid={}",
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

    if let Some(channel) = WIFI_TARGET_CHANNEL_PROBE {
        let probe = ScanConfig::default()
            .with_ssid(target_ssid)
            .with_channel(channel)
            .with_show_hidden(false)
            .with_max(WIFI_SCAN_DIAG_MAX_APS)
            .with_scan_type(ScanTypeConfig::Active {
                min: Duration::from_millis(WIFI_SCAN_ACTIVE_MIN_MS).into(),
                max: Duration::from_millis(WIFI_SCAN_ACTIVE_MAX_MS).into(),
            });
        match controller.scan_with_config_async(probe).await {
            Ok(results) if results.is_empty() => {
                println!(
                    "upload_http: scan target_ssid={} channel_probe={} found=0",
                    target_ssid, channel
                );
            }
            Ok(results) => {
                println!(
                    "upload_http: scan target_ssid={} channel_probe={} found={}",
                    target_ssid,
                    channel,
                    results.len()
                );
                for ap in results.iter() {
                    println!(
                        "upload_http: scan probe ap ssid={} channel={} rssi={} auth={:?}",
                        ap.ssid, ap.channel, ap.signal_strength, ap.auth_method
                    );
                }
                return Some(channel);
            }
            Err(err) => {
                println!(
                    "upload_http: scan target_ssid={} channel_probe={} err={:?}",
                    target_ssid, channel, err
                );
            }
        }
    }

    println!("upload_http: scan target_ssid={} found=0", target_ssid);
    None
}
