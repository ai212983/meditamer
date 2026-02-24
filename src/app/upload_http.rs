use core::cmp::min;

use embassy_futures::select::{select, Either};
use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Runner, Stack, StackResources};
use embassy_time::{with_timeout, Duration, Timer};
use embedded_io_async::Write;
use esp_hal::rng::Rng;
use esp_println::println;
use esp_radio::wifi::{
    ClientConfig, Config as WifiRuntimeConfig, ModeConfig, WifiController, WifiDevice, WifiEvent,
};
use static_cell::StaticCell;

use super::{
    config::{
        SD_UPLOAD_REQUESTS, SD_UPLOAD_RESULTS, WIFI_CONFIG_REQUESTS, WIFI_CONFIG_RESPONSES,
        WIFI_CREDENTIALS_UPDATES,
    },
    types::{
        SdUploadCommand, SdUploadRequest, SdUploadResult, SdUploadResultCode, WifiConfigRequest,
        WifiConfigResultCode, WifiCredentials, SD_PATH_MAX, SD_UPLOAD_CHUNK_MAX, WIFI_PASSWORD_MAX,
        WIFI_SSID_MAX,
    },
};

const UPLOAD_HTTP_PORT: u16 = 8080;
const HTTP_HEADER_MAX: usize = 2048;
const HTTP_RW_BUF: usize = 2048;
const SD_UPLOAD_RESPONSE_TIMEOUT_MS: u64 = 10_000;
const WIFI_CONFIG_RESPONSE_TIMEOUT_MS: u64 = 10_000;
const WIFI_RX_QUEUE_SIZE: usize = 3;
const WIFI_TX_QUEUE_SIZE: usize = 2;
const WIFI_STATIC_RX_BUF_NUM: u8 = 4;
const WIFI_DYNAMIC_RX_BUF_NUM: u16 = 8;
const WIFI_DYNAMIC_TX_BUF_NUM: u16 = 8;
const WIFI_RX_BA_WIN: u8 = 3;

#[derive(Clone, Copy)]
enum SdUploadRoundtripError {
    Timeout,
    Device(SdUploadResultCode),
}

pub(crate) struct UploadHttpRuntime {
    pub(crate) wifi_controller: WifiController<'static>,
    pub(crate) initial_credentials: Option<WifiCredentials>,
    pub(crate) net_runner: Runner<'static, WifiDevice<'static>>,
    pub(crate) stack: Stack<'static>,
}

pub(crate) fn setup(
    wifi: esp_hal::peripherals::WIFI<'static>,
) -> Result<UploadHttpRuntime, &'static str> {
    let initial_credentials = wifi_credentials().and_then(|(ssid, password)| {
        wifi_credentials_from_parts(ssid.as_bytes(), password.as_bytes()).ok()
    });

    static RADIO_CTRL: StaticCell<esp_radio::Controller<'static>> = StaticCell::new();
    static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

    let radio_ctrl = esp_radio::init().map_err(|_| "asset-upload-http: esp_radio::init failed")?;
    let radio_ctrl = RADIO_CTRL.init(radio_ctrl);
    let (wifi_controller, ifaces) = esp_radio::wifi::new(radio_ctrl, wifi, wifi_runtime_config())
        .map_err(|_| "asset-upload-http: wifi init failed")?;

    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    let (stack, net_runner) = embassy_net::new(
        ifaces.sta,
        embassy_net::Config::dhcpv4(Default::default()),
        STACK_RESOURCES.init(StackResources::<3>::new()),
        seed,
    );

    Ok(UploadHttpRuntime {
        wifi_controller,
        initial_credentials,
        net_runner,
        stack,
    })
}

#[embassy_executor::task]
pub(crate) async fn wifi_connection_task(
    mut controller: WifiController<'static>,
    mut credentials: Option<WifiCredentials>,
) {
    let mut config_applied = false;

    if let Some(sd_credentials) = load_wifi_credentials_from_sd().await {
        credentials = Some(sd_credentials);
        config_applied = false;
        println!("upload_http: loaded wifi credentials from SD");
    }

    if credentials.is_none() {
        println!("upload_http: waiting for WIFISET credentials over UART");
    }

    loop {
        while let Ok(updated) = WIFI_CREDENTIALS_UPDATES.try_receive() {
            credentials = Some(updated);
            config_applied = false;
            println!("upload_http: wifi credentials updated");
        }

        let active = match credentials {
            Some(value) => value,
            None => {
                let first = WIFI_CREDENTIALS_UPDATES.receive().await;
                credentials = Some(first);
                config_applied = false;
                println!("upload_http: wifi credentials received");
                continue;
            }
        };

        if !config_applied {
            let mode = match mode_config_from_credentials(active) {
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
                        println!("upload_http: wifi credentials changed, reconnecting");
                        let _ = controller.disconnect_async().await;
                    }
                }
            }
            Err(err) => {
                println!("upload_http: wifi connect err={:?}", err);
                let _ = controller.disconnect_async().await;
                let _ = controller.stop_async().await;
                config_applied = false;
                Timer::after(Duration::from_secs(3)).await;
            }
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
pub(crate) async fn http_server_task(stack: Stack<'static>) {
    stack.wait_config_up().await;
    if let Some(cfg) = stack.config_v4() {
        println!(
            "upload_http: listening on {}:{}",
            cfg.address.address(),
            UPLOAD_HTTP_PORT
        );
    }

    let mut rx_buffer = [0u8; HTTP_RW_BUF];
    let mut tx_buffer = [0u8; HTTP_RW_BUF];
    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    socket.set_timeout(Some(Duration::from_secs(20)));

    loop {
        let accepted = socket
            .accept(IpListenEndpoint {
                addr: None,
                port: UPLOAD_HTTP_PORT,
            })
            .await;
        if let Err(err) = accepted {
            println!("upload_http: accept err={:?}", err);
            continue;
        }

        if let Err(err) = handle_connection(&mut socket).await {
            println!("upload_http: request err={}", err);
        }

        let _ = socket.flush().await;
        Timer::after(Duration::from_millis(20)).await;
        socket.close();
        Timer::after(Duration::from_millis(20)).await;
        socket.abort();
    }
}

async fn handle_connection(socket: &mut TcpSocket<'_>) -> Result<(), &'static str> {
    let mut header_buf = [0u8; HTTP_HEADER_MAX];
    let mut filled = 0usize;
    let header_end = loop {
        if filled == header_buf.len() {
            write_response(socket, b"413 Payload Too Large", b"header too large").await;
            return Err("header too large");
        }

        let n = socket
            .read(&mut header_buf[filled..])
            .await
            .map_err(|_| "read")?;
        if n == 0 {
            return Err("eof");
        }
        filled += n;

        if let Some(end) = find_header_end(&header_buf[..filled]) {
            break end;
        }
    };

    let header = core::str::from_utf8(&header_buf[..header_end]).map_err(|_| "header utf8")?;
    let (method, target) = parse_request_line(header).ok_or("bad request line")?;
    let content_length = parse_content_length(header).unwrap_or(0);
    let body_start = header_end + 4;
    let body_bytes_in_buffer = if filled > body_start {
        filled - body_start
    } else {
        0
    };

    match (method, target_path(target)) {
        ("GET", "/health") => {
            drain_remaining_body(socket, content_length, body_bytes_in_buffer).await?;
            write_response(socket, b"200 OK", b"ok").await;
            Ok(())
        }
        ("POST", "/mkdir") => {
            drain_remaining_body(socket, content_length, body_bytes_in_buffer).await?;
            let (path, path_len) = match parse_path_query(target, "/mkdir") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            let cmd = SdUploadCommand::Mkdir { path, path_len };
            sd_upload_roundtrip_or_http_error(socket, cmd).await?;
            write_response(socket, b"200 OK", b"mkdir ok").await;
            Ok(())
        }
        ("DELETE", "/rm") => {
            drain_remaining_body(socket, content_length, body_bytes_in_buffer).await?;
            let (path, path_len) = match parse_path_query(target, "/rm") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            let cmd = SdUploadCommand::Remove { path, path_len };
            sd_upload_roundtrip_or_http_error(socket, cmd).await?;
            write_response(socket, b"200 OK", b"delete ok").await;
            Ok(())
        }
        ("POST", "/reboot") => {
            drain_remaining_body(socket, content_length, body_bytes_in_buffer).await?;
            write_response(socket, b"200 OK", b"rebooting").await;
            Timer::after(Duration::from_millis(100)).await;
            esp_hal::system::software_reset();
        }
        ("PUT", "/upload") => {
            let (path, path_len) = match parse_path_query(target, "/upload") {
                Ok(path) => path,
                Err(err) => {
                    write_response(socket, b"400 Bad Request", b"invalid path query").await;
                    return Err(err);
                }
            };
            let expected_size = content_length as u32;
            sd_upload_roundtrip_or_http_error(
                socket,
                SdUploadCommand::Begin {
                    path,
                    path_len,
                    expected_size,
                },
            )
            .await?;

            let mut sent = 0usize;
            if body_bytes_in_buffer > 0 {
                let take = min(body_bytes_in_buffer, content_length);
                let chunk = &header_buf[body_start..body_start + take];
                if let Err(err) = sd_upload_chunk(chunk).await {
                    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                    write_roundtrip_error_response(socket, err).await;
                    return Err(map_roundtrip_error_to_log(err));
                }
                sent += take;
            }

            let mut chunk_buf = [0u8; SD_UPLOAD_CHUNK_MAX];
            while sent < content_length {
                let want = min(chunk_buf.len(), content_length - sent);
                let n = socket
                    .read(&mut chunk_buf[..want])
                    .await
                    .map_err(|_| "read body")?;
                if n == 0 {
                    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                    write_response(socket, b"400 Bad Request", b"incomplete body").await;
                    return Err("incomplete body");
                }
                if let Err(err) = sd_upload_chunk(&chunk_buf[..n]).await {
                    let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                    write_roundtrip_error_response(socket, err).await;
                    return Err(map_roundtrip_error_to_log(err));
                }
                sent += n;
            }

            if let Err(err) = sd_upload_roundtrip_raw(SdUploadCommand::Commit).await {
                let _ = sd_upload_roundtrip(SdUploadCommand::Abort).await;
                write_roundtrip_error_response(socket, err).await;
                return Err(map_roundtrip_error_to_log(err));
            }
            write_response(socket, b"201 Created", b"upload ok").await;
            Ok(())
        }
        _ => {
            drain_remaining_body(socket, content_length, body_bytes_in_buffer).await?;
            write_response(socket, b"404 Not Found", b"not found").await;
            Ok(())
        }
    }
}

async fn sd_upload_chunk(data: &[u8]) -> Result<SdUploadResult, SdUploadRoundtripError> {
    if data.len() > SD_UPLOAD_CHUNK_MAX {
        return Err(SdUploadRoundtripError::Device(
            SdUploadResultCode::OperationFailed,
        ));
    }
    let mut payload = [0u8; SD_UPLOAD_CHUNK_MAX];
    payload[..data.len()].copy_from_slice(data);
    sd_upload_roundtrip_raw(SdUploadCommand::Chunk {
        data: payload,
        data_len: data.len() as u16,
    })
    .await
}

async fn sd_upload_roundtrip(command: SdUploadCommand) -> Result<SdUploadResult, &'static str> {
    match sd_upload_roundtrip_raw(command).await {
        Ok(result) => Ok(result),
        Err(err) => Err(map_roundtrip_error_to_log(err)),
    }
}

async fn sd_upload_roundtrip_or_http_error(
    socket: &mut TcpSocket<'_>,
    command: SdUploadCommand,
) -> Result<SdUploadResult, &'static str> {
    match sd_upload_roundtrip_raw(command).await {
        Ok(result) => Ok(result),
        Err(err) => {
            write_roundtrip_error_response(socket, err).await;
            Err(map_roundtrip_error_to_log(err))
        }
    }
}

async fn sd_upload_roundtrip_raw(
    command: SdUploadCommand,
) -> Result<SdUploadResult, SdUploadRoundtripError> {
    SD_UPLOAD_REQUESTS.send(SdUploadRequest { command }).await;
    let result = with_timeout(
        Duration::from_millis(SD_UPLOAD_RESPONSE_TIMEOUT_MS),
        SD_UPLOAD_RESULTS.receive(),
    )
    .await
    .map_err(|_| SdUploadRoundtripError::Timeout)?;
    if result.ok {
        Ok(result)
    } else {
        Err(SdUploadRoundtripError::Device(result.code))
    }
}

fn map_upload_code_to_error(code: SdUploadResultCode) -> &'static [u8] {
    match code {
        SdUploadResultCode::Ok => b"ok",
        SdUploadResultCode::Busy => b"sd busy",
        SdUploadResultCode::SessionNotActive => b"upload session not active",
        SdUploadResultCode::InvalidPath => b"invalid path",
        SdUploadResultCode::NotFound => b"not found",
        SdUploadResultCode::NotEmpty => b"directory not empty",
        SdUploadResultCode::SizeMismatch => b"size mismatch",
        SdUploadResultCode::PowerOnFailed => b"sd power on failed",
        SdUploadResultCode::InitFailed => b"sd init failed",
        SdUploadResultCode::OperationFailed => b"sd operation failed",
    }
}

fn map_roundtrip_error_to_log(error: SdUploadRoundtripError) -> &'static str {
    match error {
        SdUploadRoundtripError::Timeout => "sd upload timeout",
        SdUploadRoundtripError::Device(code) => match code {
            SdUploadResultCode::Ok => "ok",
            SdUploadResultCode::Busy => "sd busy",
            SdUploadResultCode::SessionNotActive => "upload session not active",
            SdUploadResultCode::InvalidPath => "invalid path",
            SdUploadResultCode::NotFound => "not found",
            SdUploadResultCode::NotEmpty => "directory not empty",
            SdUploadResultCode::SizeMismatch => "size mismatch",
            SdUploadResultCode::PowerOnFailed => "sd power on failed",
            SdUploadResultCode::InitFailed => "sd init failed",
            SdUploadResultCode::OperationFailed => "sd operation failed",
        },
    }
}

fn map_roundtrip_error_to_http_status(error: SdUploadRoundtripError) -> &'static [u8] {
    match error {
        SdUploadRoundtripError::Timeout => b"504 Gateway Timeout",
        SdUploadRoundtripError::Device(code) => match code {
            SdUploadResultCode::Ok => b"200 OK",
            SdUploadResultCode::Busy => b"409 Conflict",
            SdUploadResultCode::SessionNotActive => b"409 Conflict",
            SdUploadResultCode::InvalidPath => b"400 Bad Request",
            SdUploadResultCode::NotFound => b"404 Not Found",
            SdUploadResultCode::NotEmpty => b"409 Conflict",
            SdUploadResultCode::SizeMismatch => b"400 Bad Request",
            SdUploadResultCode::PowerOnFailed => b"503 Service Unavailable",
            SdUploadResultCode::InitFailed => b"503 Service Unavailable",
            SdUploadResultCode::OperationFailed => b"500 Internal Server Error",
        },
    }
}

async fn write_roundtrip_error_response(socket: &mut TcpSocket<'_>, error: SdUploadRoundtripError) {
    let status = map_roundtrip_error_to_http_status(error);
    let body = match error {
        SdUploadRoundtripError::Timeout => b"sd upload timeout".as_slice(),
        SdUploadRoundtripError::Device(code) => map_upload_code_to_error(code),
    };
    write_response(socket, status, body).await;
}

async fn drain_remaining_body(
    socket: &mut TcpSocket<'_>,
    content_length: usize,
    already_in_buffer: usize,
) -> Result<(), &'static str> {
    if already_in_buffer >= content_length {
        return Ok(());
    }
    let mut remaining = content_length - already_in_buffer;
    let mut sink = [0u8; 256];
    while remaining > 0 {
        let want = min(remaining, sink.len());
        let n = socket.read(&mut sink[..want]).await.map_err(|_| "drain")?;
        if n == 0 {
            return Err("drain eof");
        }
        remaining -= n;
    }
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_request_line(header: &str) -> Option<(&str, &str)> {
    let first_line = header.lines().next()?;
    let mut parts = first_line.split_ascii_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    let _version = parts.next()?;
    Some((method, target))
}

fn parse_content_length(header: &str) -> Option<usize> {
    for line in header.lines().skip(1) {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse::<usize>().ok();
        }
    }
    None
}

fn target_path(target: &str) -> &str {
    target.split('?').next().unwrap_or(target)
}

fn parse_path_query(target: &str, route: &str) -> Result<([u8; SD_PATH_MAX], u8), &'static str> {
    let query = target
        .strip_prefix(route)
        .and_then(|tail| tail.strip_prefix('?'))
        .ok_or("missing query")?;

    for pair in query.split('&') {
        if let Some(encoded) = pair.strip_prefix("path=") {
            return percent_decode_to_path_buf(encoded);
        }
    }
    Err("missing path query")
}

fn percent_decode_to_path_buf(encoded: &str) -> Result<([u8; SD_PATH_MAX], u8), &'static str> {
    let mut out = [0u8; SD_PATH_MAX];
    let mut out_len = 0usize;
    let bytes = encoded.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        let decoded = if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err("bad percent-encoding");
            }
            let hi = decode_hex(bytes[i + 1]).ok_or("bad percent-encoding")?;
            let lo = decode_hex(bytes[i + 2]).ok_or("bad percent-encoding")?;
            i += 3;
            (hi << 4) | lo
        } else if b == b'+' {
            i += 1;
            b' '
        } else {
            i += 1;
            b
        };

        if out_len >= out.len() {
            return Err("path too long");
        }
        out[out_len] = decoded;
        out_len += 1;
    }

    if out_len == 0 || out[0] != b'/' {
        return Err("path must be absolute");
    }

    Ok((out, out_len as u8))
}

fn decode_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

async fn write_response(socket: &mut TcpSocket<'_>, status: &[u8], body: &[u8]) {
    let _ = socket.write_all(b"HTTP/1.0 ").await;
    let _ = socket.write_all(status).await;
    let _ = socket.write_all(b"\r\nConnection: close\r\n\r\n").await;
    let _ = socket.write_all(body).await;
}

fn wifi_credentials() -> Option<(&'static str, &'static str)> {
    let ssid = option_env!("MEDITAMER_WIFI_SSID").or(option_env!("SSID"))?;
    let password = option_env!("MEDITAMER_WIFI_PASSWORD")
        .or(option_env!("PASSWORD"))
        .unwrap_or("");
    Some((ssid, password))
}

fn wifi_runtime_config() -> WifiRuntimeConfig {
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

fn mode_config_from_credentials(credentials: WifiCredentials) -> Option<ModeConfig> {
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).ok()?;
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).ok()?;
    Some(ModeConfig::Client(
        ClientConfig::default()
            .with_ssid(ssid.into())
            .with_password(password.into()),
    ))
}
