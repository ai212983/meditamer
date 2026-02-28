pub(crate) fn record_wifi_connect_attempt(_channel_hint: Option<u8>, _auth_idx: usize) {
    WIFI_CONNECT_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!(
        "telemetry wifi_connect_attempt auth_idx={=u8} channel_hint={=u8}",
        _auth_idx as u8,
        _channel_hint.unwrap_or(0),
    );
}

pub(crate) fn record_wifi_connect_success() {
    WIFI_CONNECT_SUCCESSES.fetch_add(1, Ordering::Relaxed);
    WIFI_LINK_CONNECTED.store(true, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::info!("telemetry wifi_connect_success");
}

pub(crate) fn record_wifi_connect_failure(reason: u8) {
    WIFI_CONNECT_FAILURES.fetch_add(1, Ordering::Relaxed);
    WIFI_LINK_CONNECTED.store(false, Ordering::Relaxed);
    if reason == 201 {
        WIFI_REASON_NO_AP_FOUND.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry wifi_connect_failure reason={=u8}", reason);
}

pub(crate) fn record_wifi_scan(result_count: usize, target_found: bool) {
    WIFI_SCAN_RUNS.fetch_add(1, Ordering::Relaxed);
    if result_count == 0 {
        WIFI_SCAN_EMPTY.fetch_add(1, Ordering::Relaxed);
    }
    if target_found {
        WIFI_SCAN_TARGET_HITS.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::debug!(
        "telemetry wifi_scan result_count={=u16} target_found={=bool}",
        result_count as u16,
        target_found,
    );
}

pub(crate) fn record_wifi_reassoc_stage(stage: u8) {
    WIFI_REASSOC_LAST_STAGE.store(stage as u32, Ordering::Relaxed);
}

pub(crate) fn record_wifi_reassoc_mode_pause() {
    WIFI_REASSOC_MODE_PAUSES.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(1);
}

pub(crate) fn record_wifi_reassoc_mode_resume() {
    WIFI_REASSOC_MODE_RESUMES.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(2);
}

pub(crate) fn record_wifi_reassoc_credentials_received() {
    WIFI_REASSOC_CREDENTIALS_RECEIVED.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(3);
}

pub(crate) fn record_wifi_reassoc_credentials_changed() {
    WIFI_REASSOC_CREDENTIALS_CHANGED.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(4);
}

pub(crate) fn record_wifi_reassoc_config_applied(
    auth_idx: usize,
    channel_hint: Option<u8>,
    probe_idx: usize,
) {
    WIFI_REASSOC_CONFIG_APPLIED.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_AUTH_IDX.store(auth_idx as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_CHANNEL_HINT.store(channel_hint.unwrap_or(0) as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_PROBE_IDX.store(probe_idx as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(5);
}

pub(crate) fn record_wifi_reassoc_start_ok() {
    WIFI_REASSOC_START_OK.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(6);
}

pub(crate) fn record_wifi_reassoc_start_err() {
    WIFI_REASSOC_START_ERR.fetch_add(1, Ordering::Relaxed);
    record_wifi_reassoc_stage(7);
}

pub(crate) fn record_wifi_reassoc_connect_begin(
    auth_idx: usize,
    channel_hint: Option<u8>,
    probe_idx: usize,
) {
    WIFI_REASSOC_CONNECT_BEGIN.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_AUTH_IDX.store(auth_idx as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_CHANNEL_HINT.store(channel_hint.unwrap_or(0) as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_PROBE_IDX.store(probe_idx as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(8);
}

pub(crate) fn record_wifi_reassoc_connect_success(elapsed_ms: u32) {
    WIFI_REASSOC_CONNECT_SUCCESS.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&WIFI_REASSOC_CONNECT_MS_TOTAL, elapsed_ms);
    update_max_u32(&WIFI_REASSOC_CONNECT_MS_MAX, elapsed_ms);
    record_wifi_reassoc_stage(9);
}

pub(crate) fn record_wifi_reassoc_connect_failure_detail(reason: u8, elapsed_ms: u32) {
    WIFI_REASSOC_CONNECT_FAILURE.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_REASON.store(reason as u32, Ordering::Relaxed);
    saturating_add_u32(&WIFI_REASSOC_CONNECT_MS_TOTAL, elapsed_ms);
    update_max_u32(&WIFI_REASSOC_CONNECT_MS_MAX, elapsed_ms);
    match reason {
        2 => WIFI_REASSOC_REASON_2.fetch_add(1, Ordering::Relaxed),
        201 => WIFI_REASSOC_REASON_201.fetch_add(1, Ordering::Relaxed),
        202 => WIFI_REASSOC_REASON_202.fetch_add(1, Ordering::Relaxed),
        203 => WIFI_REASSOC_REASON_203.fetch_add(1, Ordering::Relaxed),
        204 => WIFI_REASSOC_REASON_204.fetch_add(1, Ordering::Relaxed),
        205 => WIFI_REASSOC_REASON_205.fetch_add(1, Ordering::Relaxed),
        210 => WIFI_REASSOC_REASON_210.fetch_add(1, Ordering::Relaxed),
        211 => WIFI_REASSOC_REASON_211.fetch_add(1, Ordering::Relaxed),
        212 => WIFI_REASSOC_REASON_212.fetch_add(1, Ordering::Relaxed),
        _ => WIFI_REASSOC_REASON_OTHER.fetch_add(1, Ordering::Relaxed),
    };
    record_wifi_reassoc_stage(10);
}

pub(crate) fn record_wifi_reassoc_disconnect_event(reason: u8) {
    WIFI_REASSOC_DISCONNECT_EVENTS.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_REASON.store(reason as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(11);
}

pub(crate) fn record_wifi_reassoc_channel_probe(next_channel: u8, probe_idx: usize) {
    WIFI_REASSOC_CHANNEL_PROBES.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_CHANNEL_HINT.store(next_channel as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_PROBE_IDX.store(probe_idx as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(12);
}

pub(crate) fn record_wifi_reassoc_auth_rotation(
    auth_idx: usize,
    channel_hint: Option<u8>,
    probe_idx: usize,
) {
    WIFI_REASSOC_AUTH_ROTATIONS.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_AUTH_IDX.store(auth_idx as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_CHANNEL_HINT.store(channel_hint.unwrap_or(0) as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_PROBE_IDX.store(probe_idx as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(13);
}

pub(crate) fn record_wifi_reassoc_hint_retry(channel: u8, auth_idx: usize, probe_idx: usize) {
    WIFI_REASSOC_HINT_RETRIES.fetch_add(1, Ordering::Relaxed);
    WIFI_REASSOC_LAST_SCAN_CHANNEL.store(channel as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_CHANNEL_HINT.store(channel as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_AUTH_IDX.store(auth_idx as u32, Ordering::Relaxed);
    WIFI_REASSOC_LAST_PROBE_IDX.store(probe_idx as u32, Ordering::Relaxed);
    record_wifi_reassoc_stage(14);
}

pub(crate) fn record_wifi_reassoc_scan(
    phase: WifiScanPhase,
    result_count: usize,
    target_found: bool,
    elapsed_ms: u32,
    discovered_channel: Option<u8>,
) {
    let (runs, empty, hits, total_ms, max_ms) = match phase {
        WifiScanPhase::Active => (
            &WIFI_REASSOC_SCAN_ACTIVE_RUNS,
            &WIFI_REASSOC_SCAN_ACTIVE_EMPTY,
            &WIFI_REASSOC_SCAN_ACTIVE_HITS,
            &WIFI_REASSOC_SCAN_ACTIVE_MS_TOTAL,
            &WIFI_REASSOC_SCAN_ACTIVE_MS_MAX,
        ),
        WifiScanPhase::Passive => (
            &WIFI_REASSOC_SCAN_PASSIVE_RUNS,
            &WIFI_REASSOC_SCAN_PASSIVE_EMPTY,
            &WIFI_REASSOC_SCAN_PASSIVE_HITS,
            &WIFI_REASSOC_SCAN_PASSIVE_MS_TOTAL,
            &WIFI_REASSOC_SCAN_PASSIVE_MS_MAX,
        ),
    };
    runs.fetch_add(1, Ordering::Relaxed);
    if result_count == 0 {
        empty.fetch_add(1, Ordering::Relaxed);
    }
    if target_found {
        hits.fetch_add(1, Ordering::Relaxed);
    }
    if let Some(channel) = discovered_channel {
        WIFI_REASSOC_LAST_SCAN_CHANNEL.store(channel as u32, Ordering::Relaxed);
    }
    saturating_add_u32(total_ms, elapsed_ms);
    update_max_u32(max_ms, elapsed_ms);
    record_wifi_reassoc_stage(15);
}

pub(crate) fn record_upload_http_accept() {
    UPLOAD_HTTP_ACCEPTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!("telemetry upload_http_accept");
}

pub(crate) fn record_upload_http_accept_error() {
    UPLOAD_HTTP_ACCEPT_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_accept_error");
}

pub(crate) fn record_upload_http_accept_link_reset() {
    UPLOAD_HTTP_ACCEPT_LINK_RESETS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_accept_link_reset");
}

pub(crate) fn record_upload_http_request_error() {
    UPLOAD_HTTP_REQUEST_ERRORS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry upload_http_request_error");
}

pub(crate) fn record_upload_http_request_bucket(error: &'static str) {
    match error {
        "request header timeout" => {
            UPLOAD_HTTP_HEADER_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
        }
        "read body" => {
            UPLOAD_HTTP_READ_BODY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        "sd busy" => {
            UPLOAD_HTTP_SD_BUSY_ERRORS.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

pub(crate) fn record_upload_http_health_request() {
    UPLOAD_HTTP_HEALTH_REQUESTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::trace!("telemetry upload_http_health_request");
}

pub(crate) fn record_upload_http_upload_phase(
    bytes: u32,
    body_read_ms: u32,
    sd_wait_ms: u32,
    request_ms: u32,
) {
    UPLOAD_HTTP_UPLOAD_REQUESTS.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BYTES, bytes);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_TOTAL, body_read_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_BODY_READ_MS_MAX, body_read_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_TOTAL, sd_wait_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_SD_WAIT_MS_MAX, sd_wait_ms);
    saturating_add_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_TOTAL, request_ms);
    update_max_u32(&UPLOAD_HTTP_UPLOAD_REQUEST_MS_MAX, request_ms);
}

pub(crate) fn record_net_pipeline_dhcp_wait(elapsed_ms: u32) {
    NET_PIPELINE_DHCP_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_DHCP_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_DHCP_WAIT_MS_MAX, elapsed_ms);
}

pub(crate) fn record_net_pipeline_dhcp_ready() {
    NET_PIPELINE_DHCP_READY_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_net_pipeline_gate(reason: NetPipelineGate) {
    match reason {
        NetPipelineGate::WifiDown => {
            NET_PIPELINE_GATE_WIFI_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::LinkDown => {
            NET_PIPELINE_GATE_LINK_DOWN.fetch_add(1, Ordering::Relaxed);
        }
        NetPipelineGate::NoIpv4 => {
            NET_PIPELINE_GATE_NO_IPV4.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn record_net_pipeline_accept_wait(elapsed_ms: u32) {
    NET_PIPELINE_ACCEPT_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_TOTAL, elapsed_ms);
    update_max_u32(&NET_PIPELINE_ACCEPT_WAIT_MS_MAX, elapsed_ms);
}

pub(crate) fn record_sd_upload_roundtrip_timeout() {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    SD_UPLOAD_TIMEOUTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_roundtrip_timeout");
}

pub(crate) fn record_sd_upload_roundtrip_code(code: SdUploadResultCode) {
    SD_UPLOAD_ERRORS.fetch_add(1, Ordering::Relaxed);
    match code {
        SdUploadResultCode::Busy => {
            SD_UPLOAD_BUSY.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::PowerOnFailed => {
            SD_UPLOAD_POWER_ON_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        SdUploadResultCode::InitFailed => {
            SD_UPLOAD_INIT_FAILED.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!(
        "telemetry sd_upload_roundtrip_code code={=u8}",
        sd_upload_result_code_to_u8(code),
    );
}

pub(crate) fn record_sd_upload_roundtrip_timing(phase: SdUploadRoundtripPhase, elapsed_ms: u32) {
    let (count, total, max) = match phase {
        SdUploadRoundtripPhase::Begin => (
            &SD_UPLOAD_RTT_BEGIN_COUNT,
            &SD_UPLOAD_RTT_BEGIN_MS_TOTAL,
            &SD_UPLOAD_RTT_BEGIN_MS_MAX,
        ),
        SdUploadRoundtripPhase::Chunk => (
            &SD_UPLOAD_RTT_CHUNK_COUNT,
            &SD_UPLOAD_RTT_CHUNK_MS_TOTAL,
            &SD_UPLOAD_RTT_CHUNK_MS_MAX,
        ),
        SdUploadRoundtripPhase::Commit => (
            &SD_UPLOAD_RTT_COMMIT_COUNT,
            &SD_UPLOAD_RTT_COMMIT_MS_TOTAL,
            &SD_UPLOAD_RTT_COMMIT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Abort => (
            &SD_UPLOAD_RTT_ABORT_COUNT,
            &SD_UPLOAD_RTT_ABORT_MS_TOTAL,
            &SD_UPLOAD_RTT_ABORT_MS_MAX,
        ),
        SdUploadRoundtripPhase::Mkdir => (
            &SD_UPLOAD_RTT_MKDIR_COUNT,
            &SD_UPLOAD_RTT_MKDIR_MS_TOTAL,
            &SD_UPLOAD_RTT_MKDIR_MS_MAX,
        ),
        SdUploadRoundtripPhase::Remove => (
            &SD_UPLOAD_RTT_REMOVE_COUNT,
            &SD_UPLOAD_RTT_REMOVE_MS_TOTAL,
            &SD_UPLOAD_RTT_REMOVE_MS_MAX,
        ),
    };
    count.fetch_add(1, Ordering::Relaxed);
    saturating_add_u32(total, elapsed_ms);
    update_max_u32(max, elapsed_ms);
}

pub(crate) fn set_boot_reset_reason_code(code: Option<u8>) {
    BOOT_RESET_REASON_CODE.store(code.unwrap_or(0) as u32, Ordering::Relaxed);
}

pub(crate) fn record_sd_upload_session_timeout_abort() {
    SD_UPLOAD_SESSION_TIMEOUT_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_timeout_abort");
}

pub(crate) fn record_sd_upload_session_mode_off_abort() {
    SD_UPLOAD_SESSION_MODE_OFF_ABORTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry sd_upload_session_mode_off_abort");
}

pub(crate) fn set_wifi_link_connected(connected: bool) {
    WIFI_LINK_CONNECTED.store(connected, Ordering::Relaxed);
}

pub(crate) fn record_wifi_watchdog_disconnect() {
    WIFI_CONNECTED_WATCHDOG_DISCONNECTS.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "telemetry-defmt")]
    defmt::warn!("telemetry wifi_watchdog_disconnect");
}

pub(crate) fn wifi_link_connected() -> bool {
    WIFI_LINK_CONNECTED.load(Ordering::Relaxed)
}

pub(crate) fn set_upload_http_listener(listening: bool, ip: Option<[u8; 4]>) {
    let previous = UPLOAD_HTTP_LISTENING.swap(listening, Ordering::Relaxed);
    if listening && !previous {
        NET_PIPELINE_LISTENER_ON.fetch_add(1, Ordering::Relaxed);
    } else if !listening && previous {
        NET_PIPELINE_LISTENER_OFF.fetch_add(1, Ordering::Relaxed);
    }
    let raw_ip = ip.map(u32::from_be_bytes).unwrap_or(0);
    UPLOAD_HTTP_IPV4.store(raw_ip, Ordering::Relaxed);
}

pub(crate) fn diag_mask() -> u32 {
    DIAG_MASK.load(Ordering::Relaxed)
}

pub(crate) fn diag_enabled(domain: u32) -> bool {
    (diag_mask() & domain) != 0
}

pub(crate) fn diag_set_mask(mask: u32) -> u32 {
    let normalized = mask & DIAG_MASK_ALL;
    DIAG_MASK.store(normalized, Ordering::Relaxed);
    normalized
}

pub(crate) fn diag_set_domain(domain: u32, enabled: bool) -> u32 {
    let domain = domain & DIAG_MASK_ALL;
    let _ = DIAG_MASK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        let next = if enabled {
            current | domain
        } else {
            current & !domain
        };
        Some(next)
    });
    diag_mask()
}

fn saturating_add_u32(counter: &AtomicU32, value: u32) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

fn update_max_u32(max_counter: &AtomicU32, value: u32) {
    let mut current = max_counter.load(Ordering::Relaxed);
    while value > current {
        match max_counter.compare_exchange_weak(
            current,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

#[cfg(feature = "telemetry-defmt")]
fn sd_upload_result_code_to_u8(code: SdUploadResultCode) -> u8 {
    match code {
        SdUploadResultCode::Ok => 0,
        SdUploadResultCode::Busy => 1,
        SdUploadResultCode::SessionNotActive => 2,
        SdUploadResultCode::InvalidPath => 3,
        SdUploadResultCode::NotFound => 4,
        SdUploadResultCode::NotEmpty => 5,
        SdUploadResultCode::SizeMismatch => 6,
        SdUploadResultCode::PowerOnFailed => 7,
        SdUploadResultCode::InitFailed => 8,
        SdUploadResultCode::DirectoryFull => 9,
        SdUploadResultCode::OperationFailed => 10,
    }
}
