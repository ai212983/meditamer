use std::{path::PathBuf, process::Command, thread, time::Duration};

use anyhow::{anyhow, Context, Result};
use chrono::{Local, Utc};
use regex::Regex;
use serde_json::{json, Value};

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
    serial_console::SerialConsole,
};

#[derive(Clone, Debug)]
pub struct TroubleshootOptions {
    pub build_mode: String,
    pub output_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct TroubleshootConfig {
    flash_first: bool,
    flash_retries: u32,
    probe_retries: u32,
    probe_delay_ms: u64,
    probe_timeout_ms: u64,
    soak_cycles: u32,
}

struct TroubleshootRuntime<'a> {
    logger: &'a mut Logger,
    config: TroubleshootConfig,
    build_mode: String,
    port: String,
    baud: u32,
    uart_log_path: PathBuf,
    soak_log_dir: PathBuf,
    console: Option<SerialConsole>,
    result: String,
    failure_stage: String,
    failure_class: String,
    failure_detail: String,
    flash_ok: bool,
    probe_ok: bool,
    soak_ok: bool,
}

impl<'a> TroubleshootRuntime<'a> {
    fn new(
        logger: &'a mut Logger,
        config: TroubleshootConfig,
        build_mode: String,
        port: String,
        baud: u32,
        uart_log_path: PathBuf,
        soak_log_dir: PathBuf,
    ) -> Self {
        Self {
            logger,
            config,
            build_mode,
            port,
            baud,
            uart_log_path,
            soak_log_dir,
            console: None,
            result: "failed".to_string(),
            failure_stage: "".to_string(),
            failure_class: "".to_string(),
            failure_detail: "".to_string(),
            flash_ok: false,
            probe_ok: false,
            soak_ok: false,
        }
    }

    fn ensure_console(&mut self) -> Result<&mut SerialConsole> {
        if self.console.is_none() {
            ensure_parent_dir(&self.uart_log_path)?;
            let console = SerialConsole::open(&self.port, self.baud, Some(&self.uart_log_path))?;
            self.console = Some(console);
        }
        self.console
            .as_mut()
            .ok_or_else(|| anyhow!("failed to initialize serial console"))
    }

    fn close_console(&mut self) {
        self.console = None;
    }

    fn set_failure(
        &mut self,
        context: &mut Value,
        stage: &str,
        detail: impl Into<String>,
    ) -> Result<()> {
        let detail = detail.into();
        let class = classify_failure(stage, &detail);

        self.result = "failed".to_string();
        self.failure_stage = stage.to_string();
        self.failure_class = class.clone();
        self.failure_detail = detail;

        ctx_set_bool(context, "flash_ok", self.flash_ok)?;
        ctx_set_bool(context, "probe_ok", self.probe_ok)?;
        ctx_set_bool(context, "soak_ok", self.soak_ok)?;
        ctx_set_string(context, "result", &self.result)?;
        ctx_set_string(context, "failure_stage", &self.failure_stage)?;
        ctx_set_string(context, "failure_class", &self.failure_class)?;
        ctx_set_string(context, "failure_detail", &self.failure_detail)?;
        Ok(())
    }

    fn set_success(&mut self, context: &mut Value) -> Result<()> {
        self.result = "passed".to_string();
        self.failure_stage.clear();
        self.failure_class.clear();
        self.failure_detail.clear();

        ctx_set_bool(context, "flash_ok", self.flash_ok)?;
        ctx_set_bool(context, "probe_ok", self.probe_ok)?;
        ctx_set_bool(context, "soak_ok", self.soak_ok)?;
        ctx_set_string(context, "result", &self.result)?;
        ctx_set_string(context, "failure_stage", "")?;
        ctx_set_string(context, "failure_class", "")?;
        ctx_set_string(context, "failure_detail", "")?;
        Ok(())
    }

    fn action_preflight(&mut self, context: &mut Value) -> Result<()> {
        self.logger
            .info("Starting firmware troubleshoot workflow...");
        self.logger.info(format!(
            "port={} baud={} build_mode={} flash_first={} flash_retries={} probe_retries={} soak_cycles={}",
            self.port,
            self.baud,
            self.build_mode,
            self.config.flash_first,
            self.config.flash_retries,
            self.config.probe_retries,
            self.config.soak_cycles,
        ));
        self.logger
            .info(format!("UART log: {}", self.uart_log_path.display()));
        self.logger
            .info(format!("Soak logs: {}", self.soak_log_dir.display()));

        self.flash_ok = false;
        self.probe_ok = false;
        self.soak_ok = false;
        self.result = "failed".to_string();
        self.failure_stage.clear();
        self.failure_class.clear();
        self.failure_detail.clear();

        ctx_set_bool(context, "flash_ok", false)?;
        ctx_set_bool(context, "probe_ok", false)?;
        ctx_set_bool(context, "soak_ok", false)?;
        ctx_set_string(context, "result", "failed")?;
        ctx_set_string(context, "failure_stage", "")?;
        ctx_set_string(context, "failure_class", "")?;
        ctx_set_string(context, "failure_detail", "")?;
        Ok(())
    }

    fn action_flash_firmware(&mut self, context: &mut Value) -> Result<()> {
        if !self.config.flash_first {
            self.logger
                .info("Skipping flash step (HOSTCTL_TROUBLESHOOT_FLASH_FIRST=0)");
            self.flash_ok = true;
            ctx_set_bool(context, "flash_ok", true)?;
            return Ok(());
        }

        self.close_console();

        let script = repo_root().join("scripts/device/flash.sh");
        let repo_dir = repo_root();
        let mut last_detail = String::new();

        for attempt in 1..=self.config.flash_retries {
            self.logger.info(format!(
                "Flash attempt {attempt}/{}...",
                self.config.flash_retries
            ));
            let output = Command::new(&script)
                .arg(&self.build_mode)
                .current_dir(&repo_dir)
                .env_remove("RUSTUP_TOOLCHAIN")
                .env("ESPFLASH_PORT", &self.port)
                .env("FLASH_SET_TIME_AFTER_FLASH", "0")
                .output()
                .with_context(|| format!("failed to execute {}", script.display()));

            match output {
                Ok(output) if output.status.success() => {
                    self.flash_ok = true;
                    ctx_set_bool(context, "flash_ok", true)?;
                    self.logger.info("Flash step: PASS");
                    return Ok(());
                }
                Ok(output) => {
                    last_detail = format!(
                        "flash.sh exited with status {}\n{}",
                        output.status,
                        format_command_output(&output)
                    );
                }
                Err(err) => {
                    last_detail = format!("failed to execute flash script: {err:#}");
                }
            }

            if attempt < self.config.flash_retries {
                thread::sleep(Duration::from_secs(1));
            }
        }

        self.flash_ok = false;
        ctx_set_bool(context, "flash_ok", false)?;
        self.set_failure(context, "flash", last_detail)?;
        Ok(())
    }

    fn action_run_uart_probes(&mut self, context: &mut Value) -> Result<()> {
        self.probe_ok = false;
        ctx_set_bool(context, "probe_ok", false)?;
        let retries = self.config.probe_retries;
        let delay_ms = self.config.probe_delay_ms;
        let timeout_ms = self.config.probe_timeout_ms;

        let console = match self.ensure_console() {
            Ok(console) => console,
            Err(err) => {
                self.set_failure(context, "probe", format!("failed to open serial: {err:#}"))?;
                return Ok(());
            }
        };

        let probe_result = run_uart_probe_sequence(console, retries, delay_ms, timeout_ms);

        match probe_result {
            Ok(()) => {
                self.probe_ok = true;
                ctx_set_bool(context, "probe_ok", true)?;
                self.logger.info("UART probe step: PASS");
                Ok(())
            }
            Err(err) => {
                let detail = format!(
                    "UART probes failed: {err:#}\nRecent UART lines:\n{}",
                    recent_uart_lines(console, 20)
                );
                self.set_failure(context, "probe", detail)?;
                Ok(())
            }
        }
    }

    fn action_run_boot_soak(&mut self, context: &mut Value) -> Result<()> {
        self.soak_ok = false;
        ctx_set_bool(context, "soak_ok", false)?;
        self.close_console();

        ensure_parent_dir(&self.soak_log_dir.join("placeholder"))?;

        let script = repo_root().join("scripts/device/soak_boot.sh");
        let repo_dir = repo_root();
        self.logger.info(format!(
            "Running boot soak via {} cycles={}...",
            script.display(),
            self.config.soak_cycles
        ));

        let output = Command::new(&script)
            .arg(self.config.soak_cycles.to_string())
            .current_dir(&repo_dir)
            .env("ESPFLASH_PORT", &self.port)
            .env("SOAK_LOG_DIR", &self.soak_log_dir)
            .output()
            .with_context(|| format!("failed to execute {}", script.display()));

        match output {
            Ok(output) if output.status.success() => {
                self.soak_ok = true;
                ctx_set_bool(context, "soak_ok", true)?;
                self.logger.info("Boot soak step: PASS");
                Ok(())
            }
            Ok(output) => {
                let detail = format!(
                    "soak_boot.sh exited with status {}\n{}",
                    output.status,
                    format_command_output(&output)
                );
                self.set_failure(context, "soak", detail)?;
                Ok(())
            }
            Err(err) => {
                self.set_failure(
                    context,
                    "soak",
                    format!("failed to execute soak boot script: {err:#}"),
                )?;
                Ok(())
            }
        }
    }

    fn action_hint_uart_transport(&mut self) {
        self.logger
            .warn("UART transport hint: verify port ownership and USB serial stability.");
        self.logger.warn(format!(
            "Run: lsof {}  (look for monitor/holder processes)",
            self.port
        ));
        self.logger.warn(
            "If flaky after reset, retry with explicit HOSTCTL_PORT and keep monitor detached during flash.",
        );
    }

    fn action_hint_runtime(&mut self) {
        self.logger
            .warn("Runtime hint: panic/reset signature detected; inspect UART log around first failure marker.");
        self.logger
            .warn(format!("Look at: {}", self.uart_log_path.display()));
        self.logger.warn(
            "Focus on first panic/backtrace/stack marker rather than downstream command timeouts.",
        );
    }

    fn action_hint_dhcp_no_ipv4(&mut self) {
        self.logger.warn(
            "Wi-Fi DHCP hint: associated-without-IPv4 stall detected. Prioritize DHCP lease reacquire diagnostics before auth/scan tuning.",
        );
        self.logger.warn(
            "Use HOSTCTL_NET_POLICY_PATH (dhcp_timeout_ms / pinned_dhcp_timeout_ms) to bound stall windows per environment.",
        );
        self.logger.warn(
            "If listener is up but health fails, compare ARP/route interface and run interface-pinned /health probes from host.",
        );
    }

    fn action_mark_success(&mut self, context: &mut Value) -> Result<()> {
        self.set_success(context)
    }

    fn action_print_summary(&mut self) {
        self.logger.info("\nTroubleshoot summary");
        self.logger.info(format!("  flash_ok={}", self.flash_ok));
        self.logger.info(format!("  probe_ok={}", self.probe_ok));
        self.logger.info(format!("  soak_ok={}", self.soak_ok));
        self.logger.info(format!("  result={}", self.result));

        if self.result != "passed" {
            self.logger
                .error(format!("  failure_stage={}", self.failure_stage));
            self.logger
                .error(format!("  failure_class={}", self.failure_class));
            self.logger
                .error(format!("  failure_detail={}", self.failure_detail));
        }

        self.logger
            .info(format!("  uart_log={}", self.uart_log_path.display()));
        self.logger
            .info(format!("  soak_logs={}", self.soak_log_dir.display()));
    }
}

impl WorkflowRuntime for TroubleshootRuntime<'_> {
    fn invoke(&mut self, action: &str, _args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "preflight" => self.action_preflight(context),
            "flash_firmware" => self.action_flash_firmware(context),
            "run_uart_probes" => self.action_run_uart_probes(context),
            "run_boot_soak" => self.action_run_boot_soak(context),
            "hint_uart_transport" => {
                self.action_hint_uart_transport();
                Ok(())
            }
            "hint_runtime" => {
                self.action_hint_runtime();
                Ok(())
            }
            "hint_dhcp_no_ipv4" => {
                self.action_hint_dhcp_no_ipv4();
                Ok(())
            }
            "mark_success" => self.action_mark_success(context),
            "print_summary" => {
                self.action_print_summary();
                Ok(())
            }
            other => Err(anyhow!("unsupported troubleshoot workflow action: {other}")),
        }
    }
}

pub fn run_troubleshoot(logger: &mut Logger, opts: TroubleshootOptions) -> Result<()> {
    let flash_first = env_utils::parse_env_bool01("HOSTCTL_TROUBLESHOOT_FLASH_FIRST", true)?;
    let flash_retries = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_FLASH_RETRIES", 2)?.max(1);
    let probe_retries = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_PROBE_RETRIES", 6)?.max(1);
    let probe_delay_ms = env_utils::parse_env_u64("HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS", 700)?;
    let probe_timeout_ms = env_utils::parse_env_u64("HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS", 4000)?;
    let soak_cycles = env_utils::parse_env_u32("HOSTCTL_TROUBLESHOOT_SOAK_CYCLES", 4)?.max(1);

    let config = TroubleshootConfig {
        flash_first,
        flash_retries,
        probe_retries,
        probe_delay_ms,
        probe_timeout_ms,
        soak_cycles,
    };

    let ts = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let uart_log_path = opts
        .output_path
        .unwrap_or_else(|| PathBuf::from(format!("logs/troubleshoot_{ts}.log")));
    ensure_parent_dir(&uart_log_path)?;

    let soak_log_dir = PathBuf::from(format!("logs/troubleshoot_soak_{ts}"));
    std::fs::create_dir_all(&soak_log_dir)?;

    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;

    let workflow_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/troubleshoot.sw.yaml");
    let workflow = load_workflow(&workflow_path)?;

    let mut runtime = TroubleshootRuntime::new(
        logger,
        config,
        opts.build_mode,
        port,
        baud,
        uart_log_path,
        soak_log_dir,
    );

    let context = execute_workflow(&workflow, &mut runtime, &json!({}))?;

    if context
        .get("result")
        .and_then(Value::as_str)
        .is_some_and(|result| result == "passed")
    {
        return Ok(());
    }

    Err(anyhow!(
        "troubleshoot failed: stage={} class={}",
        runtime.failure_stage,
        runtime.failure_class
    ))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run_uart_probe_sequence(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    run_ping_probe(console, retries, delay_ms, timeout_ms)?;
    run_state_probe(console, retries, delay_ms, timeout_ms)?;
    run_timeset_probe(console, retries, delay_ms, timeout_ms)?;
    run_psram_probe(console, retries, delay_ms, timeout_ms)?;
    Ok(())
}

fn run_ping_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^PONG$")?;
    run_regex_probe(
        console,
        "PING",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing PONG",
    )
}

fn run_state_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"STATE phase=.* base=.* upload=(on|off) assets=(on|off)")?;
    run_regex_probe(
        console,
        "STATE GET",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing STATE GET response",
    )
}

fn run_psram_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^PSRAM feature_enabled=")?;
    run_regex_probe(
        console,
        "PSRAM",
        &re,
        retries,
        delay_ms,
        timeout_ms,
        "missing PSRAM response",
    )
}

fn run_timeset_probe(
    console: &mut SerialConsole,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let re = Regex::new(r"^TIMESET (OK|BUSY|ERR.*)$")?;
    let timeout = Duration::from_millis(timeout_ms.max(250));

    for _ in 0..retries {
        let mark = console.mark();
        let epoch = Utc::now().timestamp();
        console.send_line(&format!("TIMESET {epoch} 0"))?;

        if let Some(line) = console.wait_for_regex_since(mark, &re, timeout)? {
            if line.contains("TIMESET OK") {
                return Ok(());
            }
            if line.contains("TIMESET BUSY") {
                thread::sleep(Duration::from_millis(delay_ms));
                continue;
            }
            if line.contains("TIMESET ERR") {
                return Err(anyhow!("timeset probe returned ERR: {line}"));
            }
        }
        thread::sleep(Duration::from_millis(delay_ms));
    }

    Err(anyhow!("missing TIMESET OK response"))
}

fn run_regex_probe(
    console: &mut SerialConsole,
    command: &str,
    regex: &Regex,
    retries: u32,
    delay_ms: u64,
    timeout_ms: u64,
    missing_detail: &str,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms.max(250));
    for _ in 0..retries {
        let mark = console.mark();
        console.send_line(command)?;
        if console
            .wait_for_regex_since(mark, regex, timeout)?
            .is_some()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(delay_ms));
    }
    Err(anyhow!("{missing_detail}"))
}

fn recent_uart_lines(console: &SerialConsole, max_lines: usize) -> String {
    let lines = console.read_recent_lines(0);
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn format_command_output(output: &std::process::Output) -> String {
    let mut joined = String::new();

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stdout.is_empty() {
        joined.push_str("stdout:\n");
        joined.push_str(&tail_lines(&stdout, 60));
    }
    if !stderr.is_empty() {
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str("stderr:\n");
        joined.push_str(&tail_lines(&stderr, 60));
    }

    if joined.is_empty() {
        "(no command output captured)".to_string()
    } else {
        joined
    }
}

fn tail_lines(input: &str, max_lines: usize) -> String {
    let lines = input.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn classify_failure(stage: &str, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();

    if lower.contains("failed to open serial")
        || lower.contains("resource busy")
        || lower.contains("permission denied")
        || lower.contains("autodetection was not conclusive")
        || lower.contains("no such file or directory")
    {
        return "uart_transport".to_string();
    }

    if lower.contains("could not compile")
        || lower.contains("linker")
        || lower.contains("failed to run custom build")
        || lower.contains("cargo build")
    {
        return "build".to_string();
    }

    if lower.contains("flash timed out")
        || lower.contains("failed to connect")
        || lower.contains("invalid head")
        || lower.contains("espflash")
        || stage == "flash"
    {
        return "flash".to_string();
    }

    if lower.contains("guru meditation")
        || lower.contains("panic")
        || lower.contains("backtrace")
        || lower.contains("stack overflow")
        || lower.contains("stack smashing")
    {
        return "runtime".to_string();
    }

    if lower.contains("dhcp_no_ipv4_stall")
        || lower.contains("dhcp/no-ipv4 stall")
        || lower.contains("connected-without-ipv4")
    {
        return "dhcp_no_ipv4_stall".to_string();
    }

    if lower.contains("missing pong")
        || lower.contains("missing state")
        || lower.contains("missing timeset")
        || lower.contains("missing psram")
        || lower.contains("timeset err")
        || lower.contains("state err")
        || stage == "probe"
    {
        return "uart_protocol".to_string();
    }

    if lower.contains("missing:") || stage == "soak" {
        return "boot".to_string();
    }

    "unknown".to_string()
}

fn ctx_set_bool(context: &mut Value, key: &str, value: bool) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

fn ctx_set_string(context: &mut Value, key: &str, value: &str) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::classify_failure;

    #[test]
    fn classify_transport() {
        assert_eq!(
            classify_failure("probe", "failed to open serial port /dev/cu.usbserial"),
            "uart_transport"
        );
    }

    #[test]
    fn classify_runtime() {
        assert_eq!(
            classify_failure("probe", "Guru Meditation Error: Core 0 panic'ed"),
            "runtime"
        );
    }

    #[test]
    fn classify_flash_stage_defaults_to_flash() {
        assert_eq!(classify_failure("flash", "non-zero exit"), "flash");
    }

    #[test]
    fn classify_dhcp_no_ipv4_stall() {
        assert_eq!(
            classify_failure(
                "probe",
                "dhcp_no_ipv4_stall: connected-without-ipv4 observed 77 samples"
            ),
            "dhcp_no_ipv4_stall"
        );
    }
}
