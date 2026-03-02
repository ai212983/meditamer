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
