//! S3 listener lifecycle and resource placement for the shared HTTP v1 engine.
use crate::storage::http::HttpHost;
use core::sync::atomic::{AtomicU16, Ordering};
use embassy_net::{tcp::TcpSocket, Stack};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{with_timeout, Duration, Timer};
use http_upload::Host;

const RX_BYTES: usize = 65_536;
const TX_BYTES: usize = 4_096;
const CHUNK_BYTES: usize = 8_192;
pub const BUFFER_BYTES: usize =
    (RX_BYTES + TX_BYTES) * 2 + http_upload::HTTP_HEADER_MAX + CHUNK_BYTES * 2;
static BUFFERS: Mutex<CriticalSectionRawMutex, Option<&'static mut [u8]>> = Mutex::new(None);
static ACTIVE: AtomicU16 = AtomicU16::new(0);
pub fn token_configured() -> bool {
    HttpHost::upload_token().is_some_and(|token| !token.is_empty())
}
pub fn active_connections() -> u16 {
    ACTIVE.load(Ordering::Acquire)
}
struct Connection;
impl Connection {
    fn enter() -> Self {
        ACTIVE.fetch_add(1, Ordering::AcqRel);
        Self
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn install_buffers(bytes: &'static mut [u8]) {
    assert_eq!(bytes.len(), BUFFER_BYTES);
    let (storage, http) = bytes.split_at_mut(CHUNK_BYTES);
    crate::storage::init_upload_buffer(storage);
    let mut buffers = BUFFERS
        .try_lock()
        .expect("network buffers in use at startup");
    assert!(buffers.is_none(), "network buffers installed twice");
    *buffers = Some(http);
}

fn admission_open() -> bool {
    crate::net_host::is_enabled()
        && netstack::host::radio_handoff_admission_open()
        && netstack::host::listener_enabled()
}

pub async fn serve(stack: Stack<'_>) {
    let mut owner = BUFFERS.lock().await;
    let bytes = owner
        .as_mut()
        .expect("network buffers installed before owner");
    let (rx_a, bytes) = bytes.split_at_mut(RX_BYTES);
    let (tx_a, bytes) = bytes.split_at_mut(TX_BYTES);
    let (rx_b, bytes) = bytes.split_at_mut(RX_BYTES);
    let (tx_b, bytes) = bytes.split_at_mut(TX_BYTES);
    let (header, chunk) = bytes.split_at_mut(http_upload::HTTP_HEADER_MAX);
    let mut current = TcpSocket::new(stack, rx_a, tx_a);
    let mut waiting = TcpSocket::new(stack, rx_b, tx_b);
    console::println!(
        "NET_HTTP policy=token_required token_configured={} backlog=1",
        token_configured()
    );
    loop {
        if !accept(&mut current, stack).await {
            continue;
        }
        loop {
            // Keep one listener armed while the current connection handles
            // requests and sends its FIN. Only the current socket reaches SD.
            let (_, accepted) = embassy_futures::join::join(
                handle(&mut current, chunk, header),
                accept(&mut waiting, stack),
            )
            .await;
            current.abort();
            let _ = with_timeout(Duration::from_millis(250), current.flush()).await;
            if !accepted {
                waiting.abort();
                break;
            }
            core::mem::swap(&mut current, &mut waiting);
        }
    }
}

async fn accept(socket: &mut TcpSocket<'_>, stack: Stack<'_>) -> bool {
    loop {
        let ipv4 = stack
            .config_v4()
            .map(|config| config.address.address().octets());
        if !token_configured() || !admission_open() || !stack.is_link_up() || ipv4.is_none() {
            netstack::telemetry::set_upload_http_listener(false, ipv4);
            Timer::after_millis(100).await;
            continue;
        }
        socket.set_timeout(Some(Duration::from_secs(
            http_upload::HTTP_SOCKET_TIMEOUT_SECS,
        )));
        netstack::telemetry::set_upload_http_listener(true, ipv4);
        let mut pending = core::pin::pin!(socket.accept(8080));
        loop {
            match embassy_futures::select::select(pending.as_mut(), Timer::after_millis(100)).await
            {
                embassy_futures::select::Either::First(result) => return result.is_ok(),
                embassy_futures::select::Either::Second(()) => {
                    if !admission_open() || !stack.is_link_up() {
                        return false;
                    }
                }
            }
        }
    }
}

async fn handle(socket: &mut TcpSocket<'_>, chunk: &mut [u8], header: &mut [u8]) {
    let _connection = Connection::enter();
    let mut timeout = http_upload::HTTP_HEADER_READ_TIMEOUT_MS;
    while admission_open() {
        match http_upload::handle_connection::<HttpHost>(socket, chunk, header, timeout).await {
            Ok(result) if !result.connection_close_requested => {
                timeout = http_upload::HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS;
            }
            Ok(_) => break,
            Err(error) => {
                console::println!("NET_HTTP request_end={}", error);
                if crate::storage::active_roundtrips() != 0
                    || matches!(error, "read body" | "incomplete body")
                {
                    if matches!(error, "read body" | "incomplete body") {
                        socket.abort();
                    }
                    if !crate::storage::abort_upload_barrier().await {
                        console::println!("NET_HTTP error=abort_unacknowledged");
                    }
                }
                break;
            }
        }
    }
    socket.set_timeout(Some(Duration::from_millis(250)));
    let _ = socket.flush().await;
    socket.close();
    // Keep the socket registered until its FIN is transmitted and acknowledged.
    // Dropping immediately removes it before the network runner can send FIN.
    let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
}
