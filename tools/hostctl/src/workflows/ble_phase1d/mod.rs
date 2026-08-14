//! BLE Phase 1D baseline lifecycle evidence.
//!
//! This lane proves exact-artifact repeated initialization, callback-quiescent
//! shutdown, resource floors, and Wi-Fi residency. It deliberately reports the
//! forced TX/RX race and largest-block gates as incomplete.

mod analysis;
#[cfg(test)]
mod tests;

use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
    workflows::{
        common::repo_root,
        wifi::common::{acquire_port_lock, is_ready, query_net_status, PortRunLock},
    },
};
use analysis::{analyze_lines, Phase1dBaselineReport};

const PROBE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct BlePhase1dOptions {
    pub artifacts: PathBuf,
    pub board_id: String,
    pub output_path: Option<PathBuf>,
}

#[derive(Debug)]
pub(super) struct ArtifactIdentity {
    pub(super) build_id: String,
    pub(super) elf_sha256: String,
    pub(super) app_sha256: String,
    pub(super) git_head: String,
}

fn sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed reading {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn metadata_value<'a>(metadata: &'a str, key: &str) -> Option<&'a str> {
    metadata.lines().find_map(|line| {
        line.strip_prefix(key)
            .and_then(|value| value.strip_prefix('='))
    })
}

pub(super) fn validate_artifacts(root: &Path) -> Result<ArtifactIdentity> {
    let elf = root.join("firmware.elf");
    let app = root.join("app.bin");
    let metadata_path = root.join("build-metadata.txt");
    let hashes_path = root.join("sha256.txt");
    for path in [&elf, &app, &metadata_path, &hashes_path] {
        if !path.is_file() {
            bail!("missing flash-capture artifact {}", path.display());
        }
    }
    let metadata = fs::read_to_string(&metadata_path)?;
    if metadata_value(&metadata, "profile") != Some("ble-release") {
        bail!("artifact profile is not ble-release");
    }
    if metadata_value(&metadata, "image_source") != Some("build") {
        bail!("Phase 1S requires an image built in the flash-capture workflow");
    }
    if metadata_value(&metadata, "requested_features") != Some("ble-foundation")
        || metadata_value(&metadata, "no_default_features") != Some("false")
    {
        bail!("artifact must contain the canonical default-plus-ble-foundation feature set");
    }
    let build_id = metadata_value(&metadata, "firmware_build_id")
        .filter(|value| !value.is_empty() && *value != "unlabeled")
        .ok_or_else(|| anyhow!("artifact needs a non-default MEDITAMER_FIRMWARE_BUILD_ID"))?
        .to_owned();
    let git_head = metadata_value(&metadata, "git_head")
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow!("artifact metadata has no exact git HEAD"))?
        .to_owned();
    let dirty = metadata
        .split_once("git_status_begin\n")
        .and_then(|(_, rest)| rest.split_once("\ngit_status_end"))
        .map(|(status, _)| status.trim())
        .ok_or_else(|| anyhow!("artifact metadata has no git status block"))?;
    if !dirty.is_empty() {
        bail!("artifact source identity is dirty; Phase 1 requires a clean durable commit");
    }
    let elf_sha256 = sha256(&elf)?;
    let app_sha256 = sha256(&app)?;
    let recorded = fs::read_to_string(hashes_path)?;
    for (digest, name) in [(&elf_sha256, "firmware.elf"), (&app_sha256, "app.bin")] {
        let expected = format!("{digest}  {name}");
        if !recorded.lines().any(|line| line == expected) {
            bail!("artifact digest mismatch for {name}");
        }
    }
    Ok(ArtifactIdentity {
        build_id,
        elf_sha256,
        app_sha256,
        git_head,
    })
}

pub(super) fn wait_network_ready(console: &mut SerialConsole) -> Result<String> {
    for _ in 0..60 {
        if let Some(status) = query_net_status(console)? {
            if is_ready(&status, true) {
                return status
                    .ipv4
                    .ok_or_else(|| anyhow!("ready NET_STATUS omitted IPv4"));
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(anyhow!(
        "network did not become ready with listener resident"
    ))
}

fn check_health(ip: &str) -> Result<()> {
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(3))
        .build()?;
    let url = format!("http://{ip}:8080/health");
    let response = client.get(&url).send()?;
    if !response.status().is_success() {
        bail!("GET {url} returned {}", response.status());
    }
    Ok(())
}

pub(super) fn wait_running_identity(console: &mut SerialConsole, build_id: &str) -> Result<()> {
    let pong = Regex::new(r"^PONG$")?;
    let status = Regex::new(
        r"^BLEPROBE state=([a-z_]+) cycle=[0-9]+ failure=[a-z_]+ build_id=([A-Za-z0-9._-]+) cycles=([0-9]+) coex=(true|false)$",
    )?;
    for _ in 0..40 {
        if console
            .command_wait_regex("PING", &pong, Duration::from_secs(2))?
            .is_some()
        {
            if let Some(line) =
                console.command_wait_regex("BLEPROBE STATUS", &status, Duration::from_secs(2))?
            {
                let captures = status
                    .captures(&line)
                    .ok_or_else(|| anyhow!("invalid BLEPROBE STATUS response"))?;
                if &captures[2] != build_id || &captures[3] != "20" || &captures[4] != "true" {
                    bail!("running BLE identity does not match the exact artifact: {line}");
                }
                return match &captures[1] {
                    "idle" | "completed" | "failed" => Ok(()),
                    "ownership_unknown" => {
                        Err(anyhow!("BLE ownership is unknown; reboot is required"))
                    }
                    state => Err(anyhow!("BLE probe is already {state}")),
                };
            }
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(anyhow!(
        "running device did not expose the expected BLE artifact identity"
    ))
}

struct BlePhase1dRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    _port_lock: PortRunLock,
    identity: ArtifactIdentity,
    board_id: String,
    port: String,
    evidence_mark: usize,
    log_path: PathBuf,
    report: Option<Phase1dBaselineReport>,
}

impl WorkflowRuntime for BlePhase1dRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, _context: &mut Value) -> Result<()> {
        match action {
            "await_ready" => wait_running_identity(&mut self.console, &self.identity.build_id),
            "pre_health" | "post_health" => {
                let ip = wait_network_ready(&mut self.console)?;
                check_health(&ip)?;
                self.logger
                    .info(format!("{action}: GET /health passed at {ip}"));
                Ok(())
            }
            "start_probe" => {
                self.evidence_mark = self.console.mark();
                let queued = Regex::new(r"^BLEPROBE QUEUED$")?;
                self.console
                    .command_wait_regex("BLEPROBE START", &queued, Duration::from_secs(3))?
                    .ok_or_else(|| anyhow!("BLEPROBE START was not queued"))?;
                Ok(())
            }
            "await_terminal" => {
                let terminal = Regex::new(
                    r"^BLE_PHASE1D state=(completed|failed|ownership_unknown) cycle=[0-9]+ failure=[a-z_]+$",
                )?;
                self.console
                    .wait_for_regex_since(self.evidence_mark, &terminal, PROBE_TIMEOUT)?
                    .ok_or_else(|| anyhow!("BLE Phase 1D baseline timed out without retry"))?;
                Ok(())
            }
            "print_summary" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                self.logger.info(format!(
                    "BLE Phase 1D baseline passed: cycles={} min_internal={} report={}",
                    report.closed_cycles,
                    report.minimum_internal_free.unwrap_or(0),
                    self.log_path.with_extension("json").display(),
                ));
                self.logger.warn(format!(
                    "Phase 1D remains partial: {}",
                    report.remaining_gates.join("; ")
                ));
                Ok(())
            }
            "fail_evidence" => {
                let report = self
                    .report
                    .as_ref()
                    .ok_or_else(|| anyhow!("missing report"))?;
                Err(anyhow!(
                    "BLE Phase 1D baseline failed: {}",
                    report.violations.join("; ")
                ))
            }
            other => Err(anyhow!("unsupported ble-phase1d action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        if action != "analyze_evidence" {
            self.invoke(action, args, context)?;
            return Ok(None);
        }
        let lines = self.console.read_recent_lines(self.evidence_mark);
        let mut report = analyze_lines(&lines);
        report.artifact_elf_sha256 = self.identity.elf_sha256.clone();
        report.artifact_app_sha256 = self.identity.app_sha256.clone();
        report.source_git_head = self.identity.git_head.clone();
        report.build_id = self.identity.build_id.clone();
        report.board_id = self.board_id.clone();
        report.serial_port = self.port.clone();
        let report_path = self.log_path.with_extension("json");
        fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
        let baseline_passed = report.baseline_passed;
        self.report = Some(report);
        Ok(Some(json!({ "baseline_passed": baseline_passed })))
    }
}

pub fn run_ble_phase1d(logger: &mut Logger, opts: BlePhase1dOptions) -> Result<()> {
    if opts.board_id.trim().is_empty() {
        bail!("--board-id must not be empty");
    }
    let artifact_root = if opts.artifacts.is_absolute() {
        opts.artifacts
    } else {
        repo_root().join(opts.artifacts)
    };
    let identity = validate_artifacts(&artifact_root)?;
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(format!(
            "logs/ble_phase1d_baseline_{}.log",
            Local::now().format("%Y%m%d_%H%M%S")
        ))
    });
    ensure_parent_dir(&log_path)?;
    let port = env_utils::require_port()?;
    let port_lock = acquire_port_lock(&port)?;
    let baud = env_utils::baud_from_env(115_200)?;
    let console = SerialConsole::open(&port, baud, Some(&log_path))?;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/ble-phase1d.sw.yaml"),
    )?;
    let mut runtime = BlePhase1dRuntime {
        logger,
        console,
        _port_lock: port_lock,
        identity,
        board_id: opts.board_id,
        port,
        evidence_mark: 0,
        log_path,
        report: None,
    };
    execute_workflow(&workflow, &mut runtime, &json!({}))?;
    Ok(())
}
