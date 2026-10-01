use super::*;

pub(super) fn run_ble_phase1s_inner(logger: &mut Logger, opts: BlePhase1sOptions) -> Result<()> {
    validate_gate_options(&opts.board_id, opts.cycles)?;
    let telemetry_profile = TelemetryProfile::from_env()?;
    let ssid =
        std::env::var("HOSTCTL_NET_SSID").context("HOSTCTL_NET_SSID must be set (BLE Phase 1S)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios/wifi-policy.default.json")
        .display()
        .to_string();
    let policy_raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("failed reading wifi policy template: {policy_path}"))?;
    let policy = serde_json::from_str::<NetPolicy>(&policy_raw)
        .context("invalid wifi policy template JSON")?;
    enforce_policy_floors(policy, None)?;
    let netcfg_command = build_phase1s_netcfg_command(&ssid, &password, policy)?;
    if !env_utils::parse_env_bool01("HOSTCTL_NET_SKIP_HOST_WIFI_CHECK", false)? {
        ensure_host_wifi_association(&ssid)?;
    }
    let artifact_root = if opts.artifacts.is_absolute() {
        opts.artifacts
    } else {
        repo_root().join(opts.artifacts)
    };
    let identity = validate_artifacts(&artifact_root)?;
    if identity.source_dirty {
        logger.info("Phase 1S artifact metadata records source_dirty=true");
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/ble_phase1s_exclusive_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });
    ensure_parent_dir(&log_path)?;
    let payload_path = phase1s_payload_path(&log_path);
    fs::write(&payload_path, vec![0x5a; 1_024])?;
    let report_path = log_path.with_extension("json");
    let port = env_utils::require_port()?;
    let port_lock = acquire_port_lock(&port)?;
    let baud = env_utils::baud_from_env(115_200)?;
    let console = SerialConsole::open(&port, baud, Some(&log_path))?;
    let token = std::env::var("HOSTCTL_UPLOAD_TOKEN").ok();
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1s.sw.yaml"),
    )?;
    let mut runtime = BlePhase1sRuntime {
        logger,
        console,
        _port_lock: port_lock,
        identity,
        board_id: opts.board_id,
        port,
        cycles: opts.cycles,
        ssid,
        policy,
        token,
        netcfg_command,
        payload_path,
        report_path,
        evidence_mark: 0,
        boot_generation: None,
        cpu0_stack_headroom_min: None,
        touch_stack_headroom_min: None,
        serving_internal_free_min: None,
        serving_internal_min_alloc_charge: None,
        serving_internal_min_alloc_internal_required: None,
        serving_internal_min_alloc_wifi_rx_matched: None,
        serving_internal_min_alloc_correlation_stable: None,
        serving_internal_min_alloc_released: None,
        uart_log_drops_baseline: None,
        uart_log_drops_final: None,
        telemetry_profile,
        telemetry_original: None,
        telemetry_applied: None,
        telemetry_mutated: false,
        telemetry_restored: None,
        telemetry_restore_error: None,
        console_drop_samples: Vec::with_capacity(opts.cycles as usize * 6 + 2),
        samples: Vec::with_capacity(opts.cycles as usize),
        ble_samples: Vec::with_capacity(opts.cycles as usize),
        pending_cycle: None,
        known_serving_rejection: false,
        failure_stage: None,
        failure_reason: None,
        ownership_known: None,
    };
    // Telemetry cleanup covers every workflow exit, including early errors
    // and final-gate failures after a report was already persisted. Reports
    // written before this point record restored=false while a minimal
    // profile is still applied, so refresh them after the restore attempt.
    let workflow_result = execute_workflow(&workflow, &mut runtime, &json!({}));
    if let Err(error) = &workflow_result {
        if runtime.failure_reason.is_none() {
            runtime.failure_stage = Some("workflow".to_owned());
            runtime.failure_reason = Some(error.to_string());
        }
    }
    let restore_error = restore_telemetry_profile(&mut runtime).err();
    if let Some(error) = &restore_error {
        runtime.note_telemetry_restore_failure(&error.to_string());
    }
    let mut late_errors = Vec::new();
    if let Some(error) = restore_error {
        late_errors.push(format!("telemetry restore failed: {error}"));
    }
    if let Err(error) = runtime.refresh_persisted_report() {
        late_errors.push(format!("telemetry report refresh failed: {error}"));
    }
    match workflow_result {
        Ok(_) if late_errors.is_empty() => Ok(()),
        Ok(_) => Err(anyhow!("{}", late_errors.join("; "))),
        Err(workflow_error) if late_errors.is_empty() => Err(workflow_error),
        Err(workflow_error) => Err(anyhow!("{workflow_error}; {}", late_errors.join("; "))),
    }
}

const PHASE1S_REMOTE_ROOT: &str = "/assets/phase1s";
const PHASE1S_PAYLOAD_PREFIX: &str = "p1s_";
const PHASE1S_PAYLOAD_SUFFIX: &str = ".payload.bin";
const FIRMWARE_SD_PATH_MAX: usize = 64;
const _: () = assert!(
    PHASE1S_REMOTE_ROOT.len()
        + 1
        + PHASE1S_PAYLOAD_PREFIX.len()
        + 16
        + PHASE1S_PAYLOAD_SUFFIX.len()
        <= FIRMWARE_SD_PATH_MAX
);

// The upload client derives the remote `/assets/phase1s/<basename>` path from
// the local payload file name, so the basename must stay within the firmware
// path bound no matter how long the report path grows.
pub(super) fn phase1s_payload_path(log_path: &std::path::Path) -> PathBuf {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in log_path.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let name = format!("{PHASE1S_PAYLOAD_PREFIX}{hash:016x}{PHASE1S_PAYLOAD_SUFFIX}");
    match log_path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

pub(super) fn build_phase1s_netcfg_command(
    ssid: &str,
    password: &str,
    policy: NetPolicy,
) -> Result<String> {
    if policy != NetPolicy::default() {
        bail!("BLE Phase 1S compact provisioning requires the default network policy");
    }
    validate_phase1s_credential("SSID", ssid, NETCFG_SSID_MAX_BYTES, false)?;
    validate_phase1s_credential("password", password, NETCFG_PASSWORD_MAX_BYTES, true)?;
    let payload = serde_json::to_string(&json!({
        "ssid": ssid,
        "password": password,
    }))?;
    let command = format!("NETCFG SET {payload}");
    if command.len() > NETCFG_SAFE_LINE_BYTES {
        bail!(
            "BLE Phase 1S credential command exceeds the safe UART line bound: {} > {} bytes",
            command.len(),
            NETCFG_SAFE_LINE_BYTES
        );
    }
    Ok(command)
}

pub(super) fn validate_phase1s_credential(
    label: &str,
    value: &str,
    max_bytes: usize,
    empty_allowed: bool,
) -> Result<()> {
    if (!empty_allowed && value.is_empty()) || value.len() > max_bytes {
        bail!("BLE Phase 1S {label} length is outside the firmware parser bounds");
    }
    if value
        .as_bytes()
        .iter()
        .any(|byte| *byte < 0x20 || matches!(*byte, b'"' | b'\\'))
        || value.as_bytes().first() == Some(&b' ')
        || value.as_bytes().last() == Some(&b' ')
    {
        bail!("BLE Phase 1S {label} contains characters unsupported by the firmware parser");
    }
    Ok(())
}

pub(super) fn validate_gate_options(board_id: &str, cycles: u32) -> Result<()> {
    if board_id.trim().is_empty() || cycles < REQUIRED_GATE_CYCLES {
        bail!(
            "--board-id must be non-empty and --cycles must be at least {REQUIRED_GATE_CYCLES} for the Phase 1S gate"
        );
    }
    Ok(())
}
