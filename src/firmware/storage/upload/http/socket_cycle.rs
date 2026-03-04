async fn serve_connection_cycle(
    stack: Stack<'static>,
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
            Some(connection::RequestRouteKind::Mkdir)
        );
        telemetry::record_net_pipeline_accept_arm_gap(gap_us, after_mkdir);
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) && gap_us >= 500 {
            let prev_route = state
                .last_request_route
                .map(|route| route.as_str())
                .unwrap_or("unknown");
            esp_println::println!(
                "upload_http: accept_arm gap_us={} prev_route={}",
                gap_us,
                prev_route
            );
        }
    }

    let mut socket = TcpSocket::new(stack, rx_buffer.as_mut_slice(), tx_buffer.as_mut_slice());
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    telemetry::set_upload_http_listener(true, Some(local_ipv4));

    if !accept_connection(&mut socket, &stack, state).await {
        return;
    }

    let mut last_route = None;
    let mut header_timeout_ms = connection::HTTP_HEADER_READ_TIMEOUT_MS;
    loop {
        match handle_connection_request(
            &mut socket,
            chunk_buffer.as_mut_slice(),
            header_buffer.as_mut_slice(),
            header_timeout_ms,
        )
        .await
        {
            RequestHandling::Handled(route_kind) => {
                last_route = Some(route_kind);
                header_timeout_ms = connection::HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS;
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
    stack: &Stack<'static>,
    state: &mut HttpServerLoopState,
) -> bool {
    log_http_mem_diag("accept_before");
    let accept_started_at = Instant::now();
    let accepted = socket
        .accept(IpListenEndpoint {
            addr: None,
            port: UPLOAD_HTTP_PORT,
        })
        .await;
    telemetry::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));

    if let Err(err) = accepted {
        telemetry::record_upload_http_accept_error();
        if dhcp_ipv4_status(stack).is_err() {
            telemetry::record_upload_http_accept_link_reset();
            state.reset_link_state();
        }
        telemetry::set_upload_http_listener(false, None);
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!("upload_http: accept err={:?}", err);
        }
        log_http_mem_diag("accept_err");
        socket.abort();
        return false;
    }

    telemetry::record_upload_http_accept();
    log_http_mem_diag("accept_ok");
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        esp_println::println!("upload_http: accepted connection");
    }
    true
}

async fn handle_connection_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
    header_timeout_ms: u64,
) -> RequestHandling {
    log_http_mem_diag("request_begin");
    match connection::handle_connection(socket, chunk_buf, header_buf, header_timeout_ms).await {
        Ok(route_kind) => {
            log_http_mem_diag("request_ok");
            RequestHandling::Handled(route_kind)
        }
        Err(err) => {
            if matches!(
                err,
                "eof header" | "read header empty" | "read header reset empty"
            ) {
                return RequestHandling::PeerClosed;
            }
            telemetry::record_upload_http_request_error();
            telemetry::record_upload_http_request_bucket(err);
            if matches!(err, "read body" | "incomplete body") {
                // Force-close transport immediately after upload-body read failures so
                // the listener can accept the next connection without waiting for
                // graceful close semantics on a half-closed peer socket.
                socket.abort();
            }
            log_http_mem_diag("request_err");
            if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
                esp_println::println!(
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
    Handled(connection::RequestRouteKind),
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
