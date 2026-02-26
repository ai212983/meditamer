use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{with_timeout, Duration};
use static_cell::StaticCell;

mod connection;
mod helpers;

use super::super::super::types::SD_UPLOAD_CHUNK_MAX;
#[cfg(feature = "psram-alloc")]
use crate::firmware::psram;
use crate::firmware::runtime::service_mode;
use crate::firmware::telemetry;

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
const HTTP_HEADER_MAX: usize = 2048;
const HTTP_RW_BUF: usize = 2048;
const HTTP_SOCKET_TIMEOUT_SECS: u64 = 20;

enum HttpBuffer<const N: usize> {
    #[cfg(feature = "psram-alloc")]
    Psram(psram::LargeByteBuffer),
    Internal(&'static mut [u8; N]),
}

impl<const N: usize> HttpBuffer<N> {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            #[cfg(feature = "psram-alloc")]
            Self::Psram(buffer) => buffer.as_mut_slice(),
            Self::Internal(buffer) => &mut buffer[..],
        }
    }
}

fn init_http_buffer<const N: usize>(
    cell: &'static StaticCell<[u8; N]>,
    #[cfg_attr(not(feature = "psram-alloc"), allow(unused_variables))] tag: &'static str,
) -> HttpBuffer<N> {
    #[cfg(feature = "psram-alloc")]
    {
        match psram::alloc_large_byte_buffer(N) {
            Ok(buffer) => {
                esp_println::println!(
                    "upload_http: {} buffer placement={:?} bytes={}",
                    tag,
                    buffer.placement(),
                    N
                );
                psram::log_allocator_high_water(tag);
                return HttpBuffer::Psram(buffer);
            }
            Err(err) => {
                esp_println::println!(
                    "upload_http: {} psram alloc failed ({:?}); using internal ram",
                    tag,
                    err
                );
            }
        }
    }

    HttpBuffer::Internal(cell.init([0u8; N]))
}

pub(super) async fn run_http_server(stack: Stack<'static>) {
    static RX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static TX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static HEADER_BUFFER: StaticCell<[u8; HTTP_HEADER_MAX]> = StaticCell::new();
    static CHUNK_BUFFER: StaticCell<[u8; SD_UPLOAD_CHUNK_MAX]> = StaticCell::new();

    let mut rx_buffer = init_http_buffer(&RX_BUFFER, "http_rx");
    let mut tx_buffer = init_http_buffer(&TX_BUFFER, "http_tx");
    let mut header_buffer = init_http_buffer(&HEADER_BUFFER, "http_header");
    let mut chunk_buffer = init_http_buffer(&CHUNK_BUFFER, "http_chunk");

    let mut listening_logged = false;
    telemetry::set_upload_http_listener(false, None);

    loop {
        if !service_mode::upload_enabled() {
            listening_logged = false;
            telemetry::set_upload_http_listener(false, None);
            embassy_time::Timer::after(Duration::from_millis(500)).await;
            continue;
        }

        let local_ipv4 = match stack.config_v4().map(|cfg| cfg.address.address().octets()) {
            Some(ipv4) => ipv4,
            None => {
                listening_logged = false;
                telemetry::set_upload_http_listener(false, None);
                embassy_time::Timer::after(Duration::from_millis(200)).await;
                continue;
            }
        };

        if !listening_logged {
            esp_println::println!(
                "upload_http: listening on {}.{}.{}.{}:{}",
                local_ipv4[0],
                local_ipv4[1],
                local_ipv4[2],
                local_ipv4[3],
                UPLOAD_HTTP_PORT
            );
            listening_logged = true;
        }

        let mut socket = TcpSocket::new(stack, rx_buffer.as_mut_slice(), tx_buffer.as_mut_slice());
        socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
        telemetry::set_upload_http_listener(true, Some(local_ipv4));

        let accepted = socket
            .accept(IpListenEndpoint {
                addr: None,
                port: UPLOAD_HTTP_PORT,
            })
            .await;
        if let Err(err) = accepted {
            telemetry::record_upload_http_accept_error();
            telemetry::set_upload_http_listener(false, None);
            esp_println::println!("upload_http: accept err={:?}", err);
            continue;
        }

        if let Err(err) = connection::handle_connection(
            &mut socket,
            chunk_buffer.as_mut_slice(),
            header_buffer.as_mut_slice(),
        )
        .await
        {
            telemetry::record_upload_http_request_error();
            esp_println::println!("upload_http: request err={}", err);
        }

        let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
        socket.close();
        telemetry::set_upload_http_listener(false, None);
    }
}
