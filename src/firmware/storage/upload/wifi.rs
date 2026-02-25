use embassy_futures::select::{select, Either};
use embassy_time::{with_timeout, Duration, Timer};
use esp_println::println;
use esp_radio::wifi::{
    AuthMethod, ClientConfig, Config as WifiRuntimeConfig, ModeConfig, ScanConfig, ScanMethod,
    WifiController, WifiEvent,
};

use super::super::super::{
    config::{
        WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES, WIFI_CONFIG_RESPONSE_TIMEOUT_MS,
        WIFI_CREDENTIALS_UPDATES,
    },
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
const WIFI_AUTH_METHODS: [AuthMethod; 4] = [
    AuthMethod::Wpa2Personal,
    AuthMethod::WpaWpa2Personal,
    AuthMethod::Wpa2Wpa3Personal,
    AuthMethod::Wpa,
];

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
    let mut config_applied = false;
    let mut auth_method_idx = 0usize;

    if let Some(sd_credentials) = load_wifi_credentials_from_sd().await {
        credentials = Some(sd_credentials);
        config_applied = false;
        auth_method_idx = 0;
        println!("upload_http: loaded wifi credentials from SD");
    }

    if credentials.is_none() {
        println!("upload_http: waiting for WIFISET credentials over UART");
    }

    loop {
        while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
            credentials = Some(updated);
            config_applied = false;
            auth_method_idx = 0;
            println!("upload_http: wifi credentials updated");
        }

        let active = match credentials {
            Some(value) => value,
            None => {
                let first = WIFI_CREDENTIALS_UPDATES.receive().await;
                credentials = Some(first);
                config_applied = false;
                auth_method_idx = 0;
                println!("upload_http: wifi credentials received");
                continue;
            }
        };

        if !config_applied {
            let auth_method = WIFI_AUTH_METHODS[auth_method_idx];
            println!("upload_http: mode_config auth={:?}", auth_method);
            let mode = match mode_config_from_credentials(active, auth_method) {
                Some(mode) => mode,
                None => {
                    println!("upload_http: wifi credentials invalid utf8 or length");
                    credentials = None;
                    continue;
                }
            };
            println!("upload_http: station_set_config auth={:?}", auth_method);

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
                "upload_http: applying station config auth={:?}",
                auth_method
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
            }
            Err(err) => {
                println!("upload_http: wifi status err={:?}", err);
                Timer::after(Duration::from_secs(3)).await;
                continue;
            }
        }

        if WIFI_LOG_SCAN_ON_CONNECT {
            if let Ok(ssid) = core::str::from_utf8(&active.ssid[..active.ssid_len as usize]) {
                log_scan_for_target(&mut controller, ssid).await;
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
                    "upload_http: wifi connect err={:?} auth={:?}",
                    err, auth_method
                );
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                auth_method_idx = (auth_method_idx + 1) % WIFI_AUTH_METHODS.len();
                config_applied = false;
                Timer::after(Duration::from_secs(3)).await;
            }
        }
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
) -> Option<ModeConfig> {
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).ok()?;
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).ok()?;
    let auth_method = if password.is_empty() {
        AuthMethod::None
    } else {
        auth_method
    };
    Some(ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(ssid.into())
            .with_password(password.into())
            .with_auth_method(auth_method)
            .with_scan_method(ScanMethod::AllChannels),
    ))
}

async fn log_scan_for_target(controller: &mut WifiController<'static>, target_ssid: &str) {
    let config = ScanConfig::default()
        .with_ssid(target_ssid)
        .with_show_hidden(true)
        .with_max(8);
    match controller.scan_with_config_async(config).await {
        Ok(results) if results.is_empty() => {
            println!("upload_http: scan target_ssid={} found=0", target_ssid);
        }
        Ok(results) => {
            println!(
                "upload_http: scan target_ssid={} found={}",
                target_ssid,
                results.len()
            );
            for ap in results.iter() {
                println!(
                    "upload_http: scan ap ssid={} channel={} rssi={} auth={:?}",
                    ap.ssid, ap.channel, ap.signal_strength, ap.auth_method
                );
            }
        }
        Err(err) => {
            println!(
                "upload_http: scan target_ssid={} err={:?}",
                target_ssid, err
            );
        }
    }
}
