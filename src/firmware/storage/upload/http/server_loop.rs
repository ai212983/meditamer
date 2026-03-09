pub(super) async fn run_http_server(stack: Stack<'static>) {
    let mut buffers = HttpServerBuffers::new();
    let mut state = HttpServerLoopState::new();
    telemetry::set_upload_http_listener(false, None);

    loop {
        if !service_mode_ready(&mut state).await {
            continue;
        }

        let Some(local_ipv4) = gate_dhcp_ipv4(&stack, &mut state).await else {
            continue;
        };
        log_listener_start(local_ipv4, &mut state);
        buffers.ensure_initialized();

        let Some(buffers) = buffers.borrow_mut() else {
            telemetry::set_upload_http_listener(false, Some(local_ipv4));
            Timer::after(Duration::from_millis(250)).await;
            continue;
        };

        serve_connection_cycle(
            stack,
            &mut state,
            local_ipv4,
            buffers.rx,
            buffers.tx,
            buffers.header,
            buffers.chunk,
        )
        .await;
    }
}

async fn service_mode_ready(state: &mut HttpServerLoopState) -> bool {
    let app_snapshot = crate::firmware::app_state::read_app_state_snapshot();
    if !service_mode::upload_transfers_enabled() {
        let listener_enabled = service_mode::upload_http_listener_enabled();
        let listener_seq = service_mode::upload_http_listener_set_seq();
        if state.transfers_pause_started_at.is_none()
            && telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET)
        {
            esp_println::println!(
                "upload_http: transfers_paused upload_enabled={} phase={} diag_kind={} diag_targets=0x{:02x} listener_enabled={} listener_seq={}",
                app_snapshot.services.upload_enabled,
                app_snapshot.phase.as_u8(),
                app_snapshot.diag_kind.as_u8(),
                app_snapshot.diag_targets.as_persisted(),
                listener_enabled,
                listener_seq,
            );
        }
        state
            .transfers_pause_started_at
            .get_or_insert_with(Instant::now);
        state.reset_all();
        telemetry::set_upload_http_listener(false, None);
        Timer::after(Duration::from_millis(500)).await;
        return false;
    }
    if let Some(paused_started_at) = state.transfers_pause_started_at.take() {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!(
                "upload_http: transfers_resumed pause_ms={} phase={} diag_kind={}",
                elapsed_ms_u32(paused_started_at),
                app_snapshot.phase.as_u8(),
                app_snapshot.diag_kind.as_u8(),
            );
        }
    }

    let listener_enabled = service_mode::upload_http_listener_enabled();
    let listener_seq = service_mode::upload_http_listener_set_seq();
    if listener_enabled != state.listener_gate_last_enabled
        || listener_seq != state.listener_gate_last_seq
    {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!(
                "upload_http: listener_gate transition enabled={} seq={} prev_enabled={} prev_seq={}",
                listener_enabled,
                listener_seq,
                state.listener_gate_last_enabled,
                state.listener_gate_last_seq,
            );
        }
        state.listener_gate_last_enabled = listener_enabled;
        state.listener_gate_last_seq = listener_seq;
        state.listener_gate_disabled_logged = false;
    }

    if !listener_enabled {
        state.reset_all();
        telemetry::set_upload_http_listener(false, None);
        if !state.listener_gate_disabled_logged
            && telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET)
        {
            esp_println::println!(
                "upload_http: listener gate disabled; waiting for NET LISTENER ON (seq={})",
                listener_seq
            );
            state.listener_gate_disabled_logged = true;
        }
        log_http_mem_diag("listener_disabled_pause");
        Timer::after(Duration::from_millis(500)).await;
        return false;
    }

    true
}

async fn gate_dhcp_ipv4(
    stack: &Stack<'static>,
    state: &mut HttpServerLoopState,
) -> Option<[u8; 4]> {
    // Gate HTTP on active link + DHCP lease to avoid advertising an unusable listener.
    let local_ipv4 = match dhcp_ipv4_status(stack) {
        Ok(ipv4) => ipv4,
        Err(gate_reason) => {
            telemetry::record_net_pipeline_gate(gate_reason);
            if state.dhcp_gate_last_reason != Some(gate_reason) {
                let config_ipv4 = stack
                    .config_v4()
                    .map(|cfg| cfg.address.address().octets())
                    .unwrap_or([0, 0, 0, 0]);
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
                    esp_println::println!(
                        "upload_http: listener_gate reason={} wifi_connected={} link_up={} config_ipv4={}.{}.{}.{} listener_enabled={} listener_seq={}",
                        net_pipeline_gate_reason_str(gate_reason),
                        telemetry::wifi_link_connected(),
                        stack.is_link_up(),
                        config_ipv4[0],
                        config_ipv4[1],
                        config_ipv4[2],
                        config_ipv4[3],
                        service_mode::upload_http_listener_enabled(),
                        service_mode::upload_http_listener_set_seq(),
                    );
                }
                state.dhcp_gate_last_reason = Some(gate_reason);
                state.dhcp_gate_started_at = Some(Instant::now());
            } else if state.dhcp_gate_started_at.is_none() {
                state.dhcp_gate_started_at = Some(Instant::now());
            }
            state.dhcp_ready = false;
            state.listening_logged = false;
            telemetry::set_upload_http_listener(false, None);

            if !state.waiting_dhcp_logged {
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
                    esp_println::println!("upload_http: waiting for dhcp ipv4 lease");
                }
                state.waiting_dhcp_logged = true;
            }

            if state.dhcp_wait_started_at.is_none() {
                state.dhcp_wait_started_at = Some(Instant::now());
            }

            Timer::after(Duration::from_millis(DHCP_POLL_MS)).await;
            return None;
        }
    };

    if let Some(reason) = state.dhcp_gate_last_reason.take() {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            let gate_wait_ms = state
                .dhcp_gate_started_at
                .take()
                .map(elapsed_ms_u32)
                .unwrap_or(0);
            esp_println::println!(
                "upload_http: listener_gate clear reason={} wait_ms={} ipv4={}.{}.{}.{}",
                net_pipeline_gate_reason_str(reason),
                gate_wait_ms,
                local_ipv4[0],
                local_ipv4[1],
                local_ipv4[2],
                local_ipv4[3],
            );
        } else {
            state.dhcp_gate_started_at = None;
        }
    } else {
        state.dhcp_gate_started_at = None;
    }
    if let Some(started_at) = state.dhcp_wait_started_at.take() {
        telemetry::record_net_pipeline_dhcp_wait(elapsed_ms_u32(started_at));
    }
    if !state.dhcp_ready {
        telemetry::record_net_pipeline_dhcp_ready();
        state.dhcp_ready = true;
    }
    if state.waiting_dhcp_logged {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!(
                "upload_http: dhcp ipv4 ready {}.{}.{}.{}",
                local_ipv4[0],
                local_ipv4[1],
                local_ipv4[2],
                local_ipv4[3]
            );
        }
        state.waiting_dhcp_logged = false;
    }

    Some(local_ipv4)
}

fn log_listener_start(local_ipv4: [u8; 4], state: &mut HttpServerLoopState) {
    if state.listening_logged {
        return;
    }

    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
        esp_println::println!(
            "upload_http: listening on {}.{}.{}.{}:{}",
            local_ipv4[0],
            local_ipv4[1],
            local_ipv4[2],
            local_ipv4[3],
            UPLOAD_HTTP_PORT
        );
    }
    state.listening_logged = true;
}
