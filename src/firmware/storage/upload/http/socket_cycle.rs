use crate::firmware::observability;
use crate::firmware::service_mode;
use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{Duration, Instant};

use super::listener_gate::{dhcp_ipv4_status, elapsed_ms_u32};
use super::mem_diag::log_http_mem_diag;
use super::{
    ActiveConnectionGuard, HttpBuffer, HttpServerLoopState, HTTP_CHUNK_BUF_FALLBACK,
    HTTP_HEADER_MAX, HTTP_RW_BUF_FALLBACK, HTTP_SOCKET_TIMEOUT_SECS, UPLOAD_HTTP_PORT,
};
pub(super) async fn serve_connection_cycle(
    stack: Stack<'_>,
    state: &mut HttpServerLoopState,
    local_ipv4: [u8; 4],
    rx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    tx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    header_buffer: &mut HttpBuffer<HTTP_HEADER_MAX>,
    chunk_buffer: &mut HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>,
) {
    if let Some(closed_at) = state.last_request_closed_at {
        let gap_us = elapsed_us_u32(closed_at);
        let after_mkdir = matches!(
            state.last_request_route,
            Some(super::connection::RequestRouteKind::Mkdir)
        );
        observability::record_net_pipeline_accept_arm_gap(gap_us, after_mkdir);
        if observability::log_filter_enabled(observability::LOG_DOMAIN_NET) && gap_us >= 500 {
            let prev_route = state
                .last_request_route
                .map(|route| route.as_str())
                .unwrap_or("unknown");
            console::println!(
                "upload_http: accept_arm gap_us={} prev_route={}",
                gap_us,
                prev_route
            );
        }
    }

    let mut socket = TcpSocket::new(stack, rx_buffer.as_mut_slice(), tx_buffer.as_mut_slice());
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    observability::set_upload_http_listener(true, Some(local_ipv4));

    if !accept_connection(&mut socket, &stack, state).await {
        return;
    }
    let _active_connection = ActiveConnectionGuard::enter();

    let mut last_route = None;
    let mut header_timeout_ms = super::connection::HTTP_HEADER_READ_TIMEOUT_MS;
    loop {
        match handle_connection_request(
            &mut socket,
            chunk_buffer.as_mut_slice(),
            header_buffer.as_mut_slice(),
            header_timeout_ms,
        )
        .await
        {
            RequestHandling::Handled {
                route_kind,
                connection_close_requested,
            } => {
                last_route = Some(route_kind);
                if connection_close_requested {
                    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
                        console::println!(
                            "upload_http: close requested by client after route={}",
                            route_kind.as_str()
                        );
                    }
                    break;
                }
                header_timeout_ms = super::connection::HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS;
            }
            RequestHandling::PeerClosed => {
                log_http_mem_diag("request_idle_close");
                break;
            }
            RequestHandling::RequestError => {
                break;
            }
        }
    }

    // Avoid short-lived `with_timeout` wrapper timers here; use socket-level
    // timeout for a bounded best-effort flush before closing.
    socket.set_timeout(Some(Duration::from_millis(250)));
    let _ = socket.flush().await;
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    socket.close();
    log_http_mem_diag("request_close");
    state.last_request_closed_at = Some(Instant::now());
    state.last_request_route = last_route;
}

async fn accept_connection(
    socket: &mut TcpSocket<'_>,
    stack: &Stack<'_>,
    state: &mut HttpServerLoopState,
) -> bool {
    const ACCEPT_POLL_MS: u64 = 500;
    log_http_mem_diag("accept_before");
    let accept_started_at = Instant::now();
    loop {
        if !service_mode::upload_transfers_enabled() {
            observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
            state.reset_all();
            observability::set_upload_http_listener(false, None);
            if observability::log_filter_enabled(observability::LOG_DOMAIN_NET) {
                console::println!(
                    "upload_http: accept paused while transfers are disabled; re-arm later"
                );
            }
            socket.abort();
            return false;
        }
        if !service_mode::radio_handoff_admission_open() {
            observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
            state.reset_all();
            observability::set_upload_http_listener(false, None);
            socket.abort();
            return false;
        }
        if !service_mode::upload_http_listener_enabled() {
            observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
            state.reset_all();
            observability::set_upload_http_listener(false, None);
            if observability::log_filter_enabled(observability::LOG_DOMAIN_NET) {
                console::println!(
                    "upload_http: accept paused while listener gate is disabled; re-arm later"
                );
            }
            socket.abort();
            return false;
        }
        if dhcp_ipv4_status(stack).is_err() {
            observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
            observability::record_upload_http_accept_link_reset();
            state.reset_link_state();
            observability::set_upload_http_listener(false, None);
            if observability::log_filter_enabled(observability::LOG_DOMAIN_NET) {
                console::println!(
                    "upload_http: accept paused due to connectivity gate loss; re-arm later"
                );
            }
            log_http_mem_diag("accept_gate_loss");
            socket.abort();
            return false;
        }

        let endpoint = IpListenEndpoint {
            addr: None,
            port: UPLOAD_HTTP_PORT,
        };
        match embassy_time::with_timeout(
            Duration::from_millis(ACCEPT_POLL_MS),
            socket.accept(endpoint),
        )
        .await
        {
            Ok(Ok(())) => {
                observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
                observability::record_upload_http_accept();
                log_http_mem_diag("accept_ok");
                if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
                    console::println!("upload_http: accepted connection");
                }
                return true;
            }
            Ok(Err(err)) => {
                observability::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));
                observability::record_upload_http_accept_error();
                if dhcp_ipv4_status(stack).is_err() {
                    observability::record_upload_http_accept_link_reset();
                    state.reset_link_state();
                }
                observability::set_upload_http_listener(false, None);
                if observability::log_filter_enabled(observability::LOG_DOMAIN_NET) {
                    console::println!("upload_http: accept err={:?}", err);
                }
                log_http_mem_diag("accept_err");
                socket.abort();
                return false;
            }
            Err(_) => {
                // Poll link/gate state on timeout so reconnect/disconnect transitions
                // do not leave accept() parked on stale sockets indefinitely.
                continue;
            }
        }
    }
}

async fn handle_connection_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
    header_timeout_ms: u64,
) -> RequestHandling {
    log_http_mem_diag("request_begin");
    match super::connection::handle_connection(socket, chunk_buf, header_buf, header_timeout_ms)
        .await
    {
        Ok(handled) => {
            log_http_mem_diag("request_ok");
            RequestHandling::Handled {
                route_kind: handled.route_kind,
                connection_close_requested: handled.connection_close_requested,
            }
        }
        Err(err) => {
            if matches!(
                err,
                "eof header" | "read header empty" | "read header reset empty"
            ) {
                return RequestHandling::PeerClosed;
            }
            observability::record_upload_http_request_error();
            observability::record_upload_http_request_bucket(err);
            if matches!(err, "read body" | "incomplete body") {
                // Force-close transport immediately after upload-body read failures so
                // the listener can accept the next connection without waiting for
                // graceful close semantics on a half-closed peer socket.
                socket.abort();
            }
            log_http_mem_diag("request_err");
            if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
                console::println!(
                    "upload_http: request err={} recv_queue={} send_queue={} state={:?} remote={:?}",
                    err,
                    socket.recv_queue(),
                    socket.send_queue(),
                    socket.state(),
                    socket.remote_endpoint(),
                );
            }
            RequestHandling::RequestError
        }
    }
}

enum RequestHandling {
    Handled {
        route_kind: super::connection::RequestRouteKind,
        connection_close_requested: bool,
    },
    PeerClosed,
    RequestError,
}

fn elapsed_us_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_micros();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
