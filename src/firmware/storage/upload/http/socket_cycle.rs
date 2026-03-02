async fn serve_connection_cycle(
    stack: Stack<'static>,
    state: &mut HttpServerLoopState,
    local_ipv4: [u8; 4],
    rx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    tx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    header_buffer: &mut HttpBuffer<HTTP_HEADER_MAX>,
    chunk_buffer: &mut HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>,
) {
    let mut socket = TcpSocket::new(stack, rx_buffer.as_mut_slice(), tx_buffer.as_mut_slice());
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    telemetry::set_upload_http_listener(true, Some(local_ipv4));

    if !accept_connection(&mut socket, &stack, state).await {
        return;
    }

    handle_connection_request(
        &mut socket,
        chunk_buffer.as_mut_slice(),
        header_buffer.as_mut_slice(),
    )
    .await;

    let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
    socket.close();
    log_http_mem_diag("request_close");
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
        let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
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
) {
    log_http_mem_diag("request_begin");
    if let Err(err) = connection::handle_connection(socket, chunk_buf, header_buf).await {
        telemetry::record_upload_http_request_error();
        telemetry::record_upload_http_request_bucket(err);
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
    } else {
        log_http_mem_diag("request_ok");
    }
}
