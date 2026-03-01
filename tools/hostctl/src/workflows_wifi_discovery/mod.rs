mod probe;
mod profile;
mod runtime;
#[cfg(test)]
mod tests;

use std::{fs, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use serde_json::json;

use crate::{
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows_wifi_common::{preflight, MemDiagSummary, NetPolicy},
};

use probe::RoundSample;
use profile::DiscoveryProfile;

#[derive(Clone, Debug)]
pub struct WifiDiscoveryDebugOptions {
    pub output_path: Option<PathBuf>,
}

struct WifiDiscoveryRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    ssid: String,
    password: String,
    policy: NetPolicy,
    profile: DiscoveryProfile,
    samples: Vec<RoundSample>,
    ready_rounds: u32,
    zero_discovery_rounds: u32,
    ssid_seen_rounds: u32,
    total_scan_zero_events: u32,
    total_scan_nonzero_events: u32,
    total_no_ap_found_events: u32,
    mem_diag: MemDiagSummary,
}

pub fn run_wifi_discovery_debug(
    logger: &mut Logger,
    opts: WifiDiscoveryDebugOptions,
) -> Result<()> {
    let port = std::env::var("HOSTCTL_NET_PORT")
        .context("HOSTCTL_NET_PORT must be set (wifi discovery debug)")?;
    let baud = std::env::var("HOSTCTL_NET_BAUD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(115200);
    let ssid = std::env::var("HOSTCTL_NET_SSID")
        .context("HOSTCTL_NET_SSID must be set (wifi discovery debug)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = std::env::var("HOSTCTL_NET_POLICY_PATH")
        .context("HOSTCTL_NET_POLICY_PATH must be set (wifi discovery debug)")?;
    let profile_path = std::env::var("HOSTCTL_NET_DISCOVERY_PROFILE_PATH").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scenarios/wifi-discovery-debug.default.toml")
            .display()
            .to_string()
    });

    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
            format!(
                "logs/wifi_discovery_debug_{}.log",
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

    let profile_raw = fs::read_to_string(&profile_path).with_context(|| {
        format!("failed reading HOSTCTL_NET_DISCOVERY_PROFILE_PATH: {profile_path}")
    })?;
    let profile = toml::from_str::<DiscoveryProfile>(&profile_raw)
        .context("invalid TOML discovery profile")?;

    if profile.rounds == 0 {
        return Err(anyhow!("discovery profile must set rounds >= 1"));
    }

    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-discovery-debug.sw.yaml"),
    )?;

    let mut runtime = WifiDiscoveryRuntime {
        logger,
        console,
        ssid,
        password,
        policy,
        profile,
        samples: Vec::new(),
        ready_rounds: 0,
        zero_discovery_rounds: 0,
        ssid_seen_rounds: 0,
        total_scan_zero_events: 0,
        total_scan_nonzero_events: 0,
        total_no_ap_found_events: 0,
        mem_diag: MemDiagSummary::default(),
    };
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    Ok(())
}
