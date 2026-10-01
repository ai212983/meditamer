//! Bounded upload probes: acquisition coexistence and SD cancellation recovery.
use super::upload::paced::PacedUpload;
use crate::workflows::observation_fixture::{generated_request_id, wire, FixtureProvider};
use crate::{
    env_utils,
    logging::Logger,
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::{AckStatus, SerialConsole},
    workflows::wifi::common::{acquire_port_lock, detect_panic_signal, is_ready, query_net_status},
};
use anyhow::{anyhow, bail, Result};
use clap::Args;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};

const REQUEST_VALIDITY_MS: u32 = 30_000;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub enum Probe {
    Observations,
    SdRecovery,
}

impl Probe {
    fn name(self) -> &'static str {
        match self {
            Self::Observations => "observation-upload",
            Self::SdRecovery => "sd-upload-recovery",
        }
    }
}

#[derive(Debug, Args)]
pub struct Options {
    /// ELF associated with the installed image by the recorded flash evidence.
    /// This hashes that artifact; it does not read firmware back from the board.
    #[arg(long)]
    firmware_elf: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

struct Runtime {
    console: SerialConsole,
    id: u64,
    path: String,
    payload: [u8; 64],
    upload: Option<PacedUpload>,
    address: Option<SocketAddr>,
    mark: usize,
    sd_logging: Option<bool>,
    begin_count: u64,
    abort_count: u64,
    log_path: PathBuf,
    report: Value,
}

impl Runtime {
    fn await_line(&mut self, pattern: &Regex) -> Result<String> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            if let Some(upload) = self.upload.as_mut() {
                upload.pulse()?;
            }
            self.console.poll_once()?;
            if let Some(line) = self.console.find_first_regex_since(self.mark, pattern) {
                return Ok(line);
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for {pattern}");
            }
        }
    }

    fn command(&mut self, command: &str, tag: &str) -> Result<()> {
        let (status, line) = self
            .console
            .command_wait_ack(command, tag, RESPONSE_TIMEOUT)?;
        if status != AckStatus::Ok {
            bail!(
                "{command}: {}",
                line.as_deref().unwrap_or("missing acknowledgement")
            );
        }
        Ok(())
    }

    fn ready(&mut self) -> Result<()> {
        self.mark = self.console.mark();
        self.console.send_line("PING")?;
        self.await_line(&Regex::new(r"^PONG$")?)?;
        self.mark = self.console.mark();
        self.console.send_line("STATE GET")?;
        self.await_line(&Regex::new(
            r"^STATE phase=OPERATING upload=on diag_kind=NONE targets=NONE ready=true$",
        )?)?;
        let status = query_net_status(&mut self.console)?
            .ok_or_else(|| anyhow!("missing network status"))?;
        if !is_ready(&status, true) {
            bail!("network/listener is not ready; run the Wi-Fi gate first");
        }
        self.address = Some(format!("{}:8080", status.ipv4.unwrap()).parse()?);
        self.mark = self.console.mark();
        self.console.send_line("TELEM")?;
        let line = self.await_line(&Regex::new(r"^TELEM mask=")?)?;
        self.sd_logging = Some(line.split_whitespace().any(|field| field == "sd=on"));
        self.report["network_address"] = json!(self.address);
        let baseline = self.upload_metrics()?;
        self.begin_count = metric_begin_count(&baseline)?;
        self.abort_count = metric_count(&baseline, "abort_n=")?;
        self.report["upload_metrics_before"] = json!(baseline);
        Ok(())
    }

    fn upload_metrics(&mut self) -> Result<String> {
        self.mark = self.console.mark();
        self.console.send_line("METRICS")?;
        self.await_line(&Regex::new(r"^METRICS UPLOAD_RTT ")?)
    }

    fn await_upload_body(&mut self) -> Result<()> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let metrics = self.upload_metrics()?;
            if metric_begin_count(&metrics)? == self.begin_count + 1 {
                // RTT accounting happens after the SD Begin response. With final
                // body bytes withheld, this request is now in its body phase.
                self.report["upload_body_ready"] = json!(metrics);
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("SD Begin roundtrip did not complete");
            }
            self.console.settle(100)?;
        }
    }

    fn request(&mut self, provider: FixtureProvider) -> Result<()> {
        self.mark = self.console.mark();
        let id = self.id + provider.id();
        self.console.send_line(&format!(
            "OBSFIX {} {id} {REQUEST_VALIDITY_MS}",
            provider.command()
        ))?;
        let line = self.await_line(&Regex::new(&format!(r"^OBSFIX RESULT id={id}(?: |$)"))?)?;
        let result = wire::parse_result(&line, provider)?;
        self.report[provider.command()] = serde_json::to_value(&result)?;
        wire::verify_sample(&result, id, REQUEST_VALIDITY_MS, provider)?;
        Ok(())
    }

    fn sd_read(&mut self, operation: &str, pattern: &Regex) -> Result<String> {
        self.mark = self.console.mark();
        self.command(&format!("{operation} {}", self.path), operation)?;
        let id = self
            .console
            .wait_for_sdreq_id_since(self.mark, None, RESPONSE_TIMEOUT)?
            .ok_or_else(|| anyhow!("missing correlated SDREQ"))?;
        let done = Regex::new(&format!(r"^SDDONE id={id} .* status=ok code=ok(?: |$)"))?;
        self.await_line(&done)?;
        self.await_line(pattern)
    }

    fn readback(&mut self) -> Result<()> {
        let stat = self.sd_read(
            "SDFATSTAT",
            &Regex::new(&format!(
                r"^sdfat\[request\]: stat_ok path={} kind=file .* size=64$",
                regex::escape(&self.path)
            ))?,
        )?;
        let read = self.sd_read("SDFATREAD", &Regex::new(r"^sdfat\[request\]: read_ok ")?)?;
        verify_readback(&read, &self.payload)?;
        self.report["readback"] = json!({"stat": stat, "read": read, "all_bytes_match": true});
        Ok(())
    }

    fn health(&mut self) -> Result<()> {
        self.report["no_observed_runtime_fault"] = json!(false);
        self.console.settle(1_000)?;
        // The console ring can evict older lines; inspect the complete capture.
        for (index, line) in std::fs::read_to_string(&self.log_path)?.lines().enumerate() {
            if let Some(signal) = detect_panic_signal(line, index) {
                bail!("{}: {}", signal.class.as_str(), line);
            }
            if ((line.starts_with("BME688_SUSPEND") || line.starts_with("BATTERY_SUSPEND"))
                && !line.contains("outcome=Quiesced"))
                || line.contains("quiesce_failed")
                || line.contains("control_ack_timeout")
                || line.starts_with("PANEL_BUS_SUSPEND ")
                || line.starts_with("PANEL_BUS_RESUME ")
                || line.starts_with("PANEL_BUS_WAVEFORM_CLOSE ")
                || line.starts_with("PANEL_BUS_WAVEFORM_WINDOW ")
            {
                bail!("provider control failure: {line}");
            }
            if line.contains("abort recovery")
                || line.contains("autonomous_upload_abort_failed")
                || (line.starts_with("sd_upload: abort ") && line.contains("failed"))
            {
                bail!("upload abort failure: {line}");
            }
        }
        self.report["no_observed_runtime_fault"] = json!(true);
        Ok(())
    }

    fn verify_upload_identity(&mut self) -> Result<()> {
        let metrics = self.upload_metrics()?;
        if metric_begin_count(&metrics)? != self.begin_count + 1 {
            bail!("another upload interfered with the Begin correlation");
        }
        self.report["upload_metrics_after"] = json!(metrics);
        Ok(())
    }

    fn await_abort(&mut self) -> Result<()> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            let metrics = self.upload_metrics()?;
            if metric_begin_count(&metrics)? != self.begin_count + 1 {
                bail!("another upload interfered with cancellation correlation");
            }
            let count = metric_count(&metrics, "abort_n=")?;
            if count == self.abort_count + 1 {
                self.report["cancellation"]["abort_metrics"] = json!(metrics);
                return Ok(());
            }
            if count > self.abort_count + 1 || Instant::now() >= deadline {
                bail!("missing or ambiguous upload Abort roundtrip");
            }
            self.console.settle(100)?;
        }
    }

    fn stat_absent(&mut self, path: &str) -> Result<String> {
        self.mark = self.console.mark();
        self.command(&format!("SDFATSTAT {path}"), "SDFATSTAT")?;
        let id = self
            .console
            .wait_for_sdreq_id_since(self.mark, None, RESPONSE_TIMEOUT)?
            .ok_or_else(|| anyhow!("missing correlated SDREQ for {path}"))?;
        let done = self.await_line(&Regex::new(&format!(r"^SDDONE id={id}(?: |$)"))?)?;
        if !done.contains(" status=error code=not_found ") {
            bail!("cancelled upload left a file at {path}: {done}");
        }
        Ok(done)
    }

    fn verify_cancelled_files(&mut self) -> Result<()> {
        let path = self.path.clone();
        let final_stat = self.stat_absent(&path)?;
        let temp_stat = self.stat_absent("/assets/HCTLUPLD.TMP")?;
        self.report["cancellation"]["final_absent"] = json!(final_stat);
        self.report["cancellation"]["temp_absent"] = json!(temp_stat);
        // Use a distinct destination, but the same SD runner and mount.
        self.begin_count += 1;
        self.path = format!("/assets/recover{:x}.bin", self.id);
        self.report["path"] = json!(self.path);
        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        self.upload = None; // Close an incomplete body; the firmware aborts it on EOF.
        if let Some(enabled) = self.sd_logging {
            self.command(
                if enabled {
                    "TELEMSET SD ON"
                } else {
                    "TELEMSET SD OFF"
                },
                "TELEMSET",
            )?;
        }
        Ok(())
    }
}

impl WorkflowRuntime for Runtime {
    fn invoke(&mut self, action: &str, _: &Value, _: &mut Value) -> Result<()> {
        match action {
            "check_ready" => self.ready(),
            "enable_sd_logging" => self.command("TELEMSET SD ON", "TELEMSET"),
            "start_upload" => {
                self.mark = self.console.mark();
                self.upload = Some(PacedUpload::start(
                    self.address.ok_or_else(|| anyhow!("missing address"))?,
                    &self.path,
                    std::env::var("HOSTCTL_UPLOAD_TOKEN").ok().as_deref(),
                    self.payload,
                )?);
                Ok(())
            }
            "await_upload_begin" => {
                let line = self.await_line(&Regex::new(&format!(
                    r"^sd_upload: begin path={} expected_size=64$",
                    regex::escape(&self.path)
                ))?)?;
                self.report["upload_begin"] = json!(line);
                Ok(())
            }
            "request_environment" => self.request(FixtureProvider::Bme688),
            "await_upload_body" => self.await_upload_body(),
            "request_battery" => self.request(FixtureProvider::Battery),
            "cancel_upload" => {
                let upload = self
                    .upload
                    .take()
                    .ok_or_else(|| anyhow!("missing upload"))?;
                self.report["cancellation"] = json!({"path": self.path,
                    "begin": self.report["upload_begin"], "body_ready": self.report["upload_body_ready"],
                    "transport": upload.cancel()?});
                Ok(())
            }
            "await_abort" => self.await_abort(),
            "verify_cancelled_files" => self.verify_cancelled_files(),
            "finish_upload" => {
                self.report["transfer"] = self
                    .upload
                    .as_mut()
                    .ok_or_else(|| anyhow!("missing upload"))?
                    .finish()?;
                self.upload = None;
                Ok(())
            }
            "verify_readback" => self.readback(),
            "verify_upload_identity" => self.verify_upload_identity(),
            "check_health" => self.health(),
            "cleanup" => self.cleanup(),
            other => bail!("unknown upload probe action: {other}"),
        }
    }
}

fn verify_readback(line: &str, payload: &[u8; 64]) -> Result<()> {
    let hex: String = payload.iter().map(|byte| format!("{byte:02x}")).collect();
    if line != format!("sdfat[request]: read_ok bytes=64 preview_hex={hex}") {
        bail!("SD readback does not match all 64 uploaded bytes");
    }
    Ok(())
}

fn metric_begin_count(line: &str) -> Result<u64> {
    metric_count(line, "begin_n=")
}

fn metric_count(line: &str, prefix: &str) -> Result<u64> {
    line.split_whitespace()
        .find_map(|field| field.strip_prefix(prefix))
        .ok_or_else(|| anyhow!("missing upload counter {prefix}"))?
        .parse()
        .map_err(Into::into)
}

pub fn run(logger: &mut Logger, options: Options, probe: Probe) -> Result<()> {
    let firmware_sha = format!(
        "{:x}",
        Sha256::digest(std::fs::read(&options.firmware_elf)?)
    );
    let id = generated_request_id()?
        .checked_sub(2)
        .ok_or_else(|| anyhow!("invalid request identity"))?;
    let port = env_utils::require_port()?;
    let _lock = acquire_port_lock(&port)?;
    std::fs::create_dir_all(&options.output)?;
    let log_path = options.output.join("serial.log");
    let mut console =
        SerialConsole::open_passive(&port, env_utils::baud_from_env(115_200)?, Some(&log_path))?;
    console.set_read_timeout(Duration::from_millis(100))?;
    let mut runtime = Runtime {
        console,
        id,
        path: format!("/assets/obs{id:x}.bin"),
        payload: std::array::from_fn(|i| ((i * 17 + 31) as u8) ^ (id >> ((i % 8) * 8)) as u8),
        upload: None,
        address: None,
        mark: 0,
        sd_logging: None,
        begin_count: 0,
        abort_count: 0,
        log_path,
        report: json!({"firmware_elf_sha256": firmware_sha,
        "identity_source": "operator-associated ELF; use the recorded flash evidence, not device readback",
        "scope": match probe {
            Probe::Observations => "64-byte paced HTTP upload; acquisition during body ingestion, not sustained SD writes",
            Probe::SdRecovery => "incomplete HTTP body cancellation, SD temporary-file cleanup, then a new complete upload/readback; not in-flight DMA cancellation",
        }}),
    };
    runtime.report["path"] = json!(runtime.path);
    std::fs::write(options.output.join("payload.bin"), runtime.payload)?;
    runtime.report["payload_sha256"] = json!(format!("{:x}", Sha256::digest(runtime.payload)));
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("scenarios/{}.sw.yaml", probe.name())),
    )?;
    let result = execute_workflow(&workflow, &mut runtime, &json!({"passed": false}));
    let passed = result
        .as_ref()
        .is_ok_and(|context| context["passed"] == true && context.get("cleanup_error").is_none());
    runtime.report["passed"] = json!(passed);
    runtime.report["workflow"] = match &result {
        Ok(context) => context.clone(),
        Err(error) => json!({"passed": false, "error": format!("{error:#}")}),
    };
    std::fs::write(
        options.output.join("report.json"),
        serde_json::to_vec_pretty(&runtime.report)?,
    )?;
    result?;
    if !passed {
        bail!(
            "{} failed; see {}/report.json",
            probe.name(),
            options.output.display()
        );
    }
    logger.info(format!(
        "{} passed; report={}/report.json",
        probe.name(),
        options.output.display()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_requires_both_samples_before_final_bytes_and_always_cleans_up() -> Result<()> {
        struct Recording {
            actions: Vec<String>,
            fail: Option<&'static str>,
        }
        impl WorkflowRuntime for Recording {
            fn invoke(&mut self, action: &str, _: &Value, _: &mut Value) -> Result<()> {
                self.actions.push(action.into());
                if self.fail == Some(action) {
                    bail!("injected failure");
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/observation-upload.sw.yaml"),
        )?;
        let expected = [
            "check_ready",
            "enable_sd_logging",
            "start_upload",
            "await_upload_begin",
            "await_upload_body",
            "request_environment",
            "request_battery",
            "finish_upload",
            "verify_upload_identity",
            "verify_readback",
            "check_health",
        ];
        for fail in std::iter::once(None).chain(expected.iter().map(|action| Some(*action))) {
            let mut runtime = Recording {
                actions: Vec::new(),
                fail,
            };
            let result = execute_workflow(&workflow, &mut runtime, &json!({"passed": false}))?;
            assert_eq!(result["passed"], fail.is_none());
            let count = fail
                .map(|action| expected.iter().position(|item| *item == action).unwrap() + 1)
                .unwrap_or(expected.len());
            assert_eq!(&runtime.actions[..count], &expected[..count]);
            assert_eq!(&runtime.actions[count..], &["cleanup", "check_health"]);
            assert_eq!(runtime.actions.len(), count + 2);
        }
        Ok(())
    }
    #[test]
    fn full_readback_rejects_short_corrupt_and_extra_data() {
        let bytes = [0xab; 64];
        let valid = format!(
            "sdfat[request]: read_ok bytes=64 preview_hex={}",
            "ab".repeat(64)
        );
        assert!(verify_readback(&valid, &bytes).is_ok());
        for invalid in [
            valid.replace("bytes=64", "bytes=65"),
            valid.replacen("ab", "ac", 1),
            valid[..valid.len() - 2].into(),
            format!("{valid}ab"),
        ] {
            assert!(verify_readback(&invalid, &bytes).is_err());
        }
    }

    #[test]
    fn recovery_workflow_requires_abort_and_cleanup_before_new_upload() -> Result<()> {
        let expected = [
            "check_ready",
            "enable_sd_logging",
            "start_upload",
            "await_upload_begin",
            "await_upload_body",
            "cancel_upload",
            "await_abort",
            "verify_cancelled_files",
            "start_upload",
            "await_upload_begin",
            "await_upload_body",
            "finish_upload",
            "verify_upload_identity",
            "verify_readback",
            "check_health",
        ];
        struct Recording {
            actions: Vec<String>,
            fail_at: Option<usize>,
        }
        impl WorkflowRuntime for Recording {
            fn invoke(&mut self, action: &str, _: &Value, _: &mut Value) -> Result<()> {
                let index = self.actions.len();
                self.actions.push(action.into());
                if self.fail_at == Some(index) {
                    bail!("injected failure");
                }
                Ok(())
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/sd-upload-recovery.sw.yaml"),
        )?;
        for fail_at in std::iter::once(None).chain((0..expected.len() + 2).map(Some)) {
            let mut runtime = Recording {
                actions: Vec::new(),
                fail_at,
            };
            let result = execute_workflow(&workflow, &mut runtime, &json!({"passed": false}))?;
            let passed = result["passed"] == true && result.get("cleanup_error").is_none();
            assert_eq!(passed, fail_at.is_none());
            let count = fail_at
                .filter(|n| *n < expected.len())
                .map(|n| n + 1)
                .unwrap_or(expected.len());
            assert_eq!(&runtime.actions[..count], &expected[..count]);
            assert_eq!(runtime.actions[count], "cleanup");
            if fail_at != Some(expected.len()) {
                assert_eq!(runtime.actions[count + 1], "check_health");
            }
        }
        Ok(())
    }
}
