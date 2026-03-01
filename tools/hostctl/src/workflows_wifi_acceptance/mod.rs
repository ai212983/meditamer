mod boot_gate;
mod host_wifi;
mod runtime_core;
mod runtime_upload;
mod wait_ready;

use std::{fs, path::PathBuf, time::Instant};

use anyhow::{Context, Result};
use serde_json::json;

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows_wifi_common::{preflight, MemDiagSummary, NetPolicy},
};

use boot_gate::{run_boot_discovery_gate, BootDiscoveryGateConfig};
use host_wifi::ensure_host_wifi_association;

#[derive(Clone, Debug)]
pub struct WifiAcceptanceOptions {
    pub output_path: Option<PathBuf>,
}

struct WifiAcceptanceRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    payload_path: PathBuf,
    remote_root: String,
    ssid: String,
    password: String,
    token: Option<String>,
    policy: NetPolicy,
    cycles: u32,
    operation_retries: u32,
    connect_samples: Vec<f64>,
    listen_samples: Vec<f64>,
    upload_samples: Vec<f64>,
    throughput_samples: Vec<f64>,
    started: Instant,
    mem_diag: MemDiagSummary,
    mem_read_mark: usize,
}

pub fn run_wifi_acceptance(logger: &mut Logger, opts: WifiAcceptanceOptions) -> Result<()> {
    let port = std::env::var("HOSTCTL_NET_PORT")
        .context("HOSTCTL_NET_PORT must be set (hard-cut net workflow)")?;
    let baud = std::env::var("HOSTCTL_NET_BAUD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(115200);
    let ssid = std::env::var("HOSTCTL_NET_SSID")
        .context("HOSTCTL_NET_SSID must be set (hard-cut net workflow)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = std::env::var("HOSTCTL_NET_POLICY_PATH")
        .context("HOSTCTL_NET_POLICY_PATH must be set (hard-cut net workflow)")?;
    let skip_host_wifi_check =
        env_utils::parse_env_bool01("HOSTCTL_NET_SKIP_HOST_WIFI_CHECK", false)?;
    if !skip_host_wifi_check {
        ensure_host_wifi_association(&ssid)?;
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
            format!(
                "logs/wifi_acceptance_{}.log",
                chrono::Local::now().format("%Y%m%d_%H%M%S")
            )
        }))
    });

    ensure_parent_dir(&log_path)?;
    let mut console = SerialConsole::open(&port, baud, Some(&log_path))?;
    preflight(&mut console)?;

    let policy_raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("failed reading HOSTCTL_NET_POLICY_PATH: {policy_path}"))?;
    let policy = serde_json::from_str::<NetPolicy>(&policy_raw)
        .context("invalid HOSTCTL_NET_POLICY_PATH JSON")?;
    let cycles = env_utils::parse_env_u32("HOSTCTL_NET_CYCLES", 3)?.max(1);
    let operation_retries = env_utils::parse_env_u32("HOSTCTL_NET_OPERATION_RETRIES", 3)?.max(1);
    let require_boot_discovery_gate =
        env_utils::parse_env_bool01("HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE", true)?;
    let boot_discovery_cfg = BootDiscoveryGateConfig {
        max_boot_uptime_ms: env_utils::parse_env_u32(
            "HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS",
            30_000,
        )?,
        timeout_ms: env_utils::parse_env_u32("HOSTCTL_NET_BOOT_DISCOVERY_TIMEOUT_MS", 180_000)?,
        settle_ms: env_utils::parse_env_u32("HOSTCTL_NET_BOOT_DISCOVERY_SETTLE_MS", 6_000)?,
        allow_ready_only_fallback: env_utils::parse_env_bool01(
            "HOSTCTL_NET_BOOT_DISCOVERY_READY_ONLY_FALLBACK",
            false,
        )?,
    };

    if require_boot_discovery_gate {
        run_boot_discovery_gate(
            logger,
            &mut console,
            &ssid,
            &password,
            policy,
            boot_discovery_cfg,
        )?;
    }

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml"),
    )?;

    let payload_path = PathBuf::from("/tmp/net_acceptance_payload.bin");
    let remote_root = "/assets".to_string();
    let token = std::env::var("HOSTCTL_UPLOAD_TOKEN").ok();

    let mut runtime = WifiAcceptanceRuntime {
        logger,
        console,
        payload_path,
        remote_root,
        ssid,
        password,
        token,
        policy,
        cycles,
        operation_retries,
        connect_samples: Vec::new(),
        listen_samples: Vec::new(),
        upload_samples: Vec::new(),
        throughput_samples: Vec::new(),
        started: Instant::now(),
        mem_diag: MemDiagSummary::default(),
        mem_read_mark: 0,
    };
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    Ok(())
}
