use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{with_timeout, Duration};
use static_cell::StaticCell;

mod connection;
mod helpers;

use super::super::super::types::SD_UPLOAD_CHUNK_MAX;

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
const HTTP_HEADER_MAX: usize = 2048;
const HTTP_RW_BUF: usize = 2048;

pub(super) async fn run_http_server(stack: Stack<'static>) {
    static RX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static TX_BUFFER: StaticCell<[u8; HTTP_RW_BUF]> = StaticCell::new();
    static CHUNK_BUFFER: StaticCell<[u8; SD_UPLOAD_CHUNK_MAX]> = StaticCell::new();

    let rx_buffer = RX_BUFFER.init([0u8; HTTP_RW_BUF]);
    let tx_buffer = TX_BUFFER.init([0u8; HTTP_RW_BUF]);
    let chunk_buffer = CHUNK_BUFFER.init([0u8; SD_UPLOAD_CHUNK_MAX]);

    stack.wait_config_up().await;
    if let Some(cfg) = stack.config_v4() {
        esp_println::println!(
            "upload_http: listening on {}:{}",
            cfg.address.address(),
            UPLOAD_HTTP_PORT
        );
    }

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer[..], &mut tx_buffer[..]);
        socket.set_timeout(Some(Duration::from_secs(20)));

        let accepted = socket
            .accept(IpListenEndpoint {
                addr: None,
                port: UPLOAD_HTTP_PORT,
            })
            .await;
        if let Err(err) = accepted {
            esp_println::println!("upload_http: accept err={:?}", err);
            continue;
        }

        if let Err(err) = connection::handle_connection(&mut socket, chunk_buffer).await {
            esp_println::println!("upload_http: request err={}", err);
        }

        let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
        socket.close();
    }
}
