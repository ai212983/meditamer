use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::{
    logging::{ensure_parent_dir, Logger},
    scenarios::WorkflowRuntime,
    serial_console::SerialConsole,
};

use super::{
    classify::{classify_failure, runtime_subclass},
    context::{ctx_set_bool, ctx_set_string},
    TroubleshootConfig, TroubleshootRuntime,
};

impl<'a> TroubleshootRuntime<'a> {
    pub(super) fn new(
        logger: &'a mut Logger,
        config: TroubleshootConfig,
        build_mode: String,
        port: String,
        baud: u32,
        uart_log_path: std::path::PathBuf,
        soak_log_dir: std::path::PathBuf,
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

    pub(super) fn ensure_console(&mut self) -> Result<&mut SerialConsole> {
        if self.console.is_none() {
            ensure_parent_dir(&self.uart_log_path)?;
            let console = SerialConsole::open(&self.port, self.baud, Some(&self.uart_log_path))?;
            self.console = Some(console);
        }
        self.console
            .as_mut()
            .ok_or_else(|| anyhow!("failed to initialize serial console"))
    }

    pub(super) fn close_console(&mut self) {
        self.console = None;
    }

    fn build_status_result(&self) -> Value {
        json!({
            "flash_ok": self.flash_ok,
            "probe_ok": self.probe_ok,
            "soak_ok": self.soak_ok,
            "result": self.result,
            "failure_stage": self.failure_stage,
            "failure_class": self.failure_class,
            "failure_detail": self.failure_detail
        })
    }

    fn build_preflight_result(&self) -> Value {
        json!({
            "flash_ok": self.flash_ok,
            "probe_ok": self.probe_ok,
            "soak_ok": self.soak_ok,
            "flash_retry_count": self.config.flash_retries.saturating_sub(1),
            "flash_retry_delay_ms": 1_000,
            "probe_retry_count": self.config.probe_retries.saturating_sub(1),
            "probe_retry_delay_ms": self.config.probe_delay_ms as u32,
            "result": self.result,
            "failure_stage": self.failure_stage,
            "failure_class": self.failure_class,
            "failure_detail": self.failure_detail
        })
    }

    pub(super) fn set_failure(
        &mut self,
        context: &mut Value,
        stage: &str,
        detail: impl Into<String>,
    ) -> Result<()> {
        let detail = detail.into();
        let class = classify_failure(stage, &detail);
        let detail = if class == "runtime" {
            if let Some(subclass) = runtime_subclass(&detail) {
                format!("runtime_subclass={subclass}; {detail}")
            } else {
                detail
            }
        } else {
            detail
        };

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

    fn set_success(&mut self) {
        self.result = "passed".to_string();
        self.failure_stage.clear();
        self.failure_class.clear();
        self.failure_detail.clear();
    }

    fn action_preflight(&mut self) {
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
        self.logger.warn(
            "Runtime hint: panic/reset signature detected; inspect UART log around first failure marker.",
        );
        self.logger
            .warn(format!("Look at: {}", self.uart_log_path.display()));
        self.logger.warn(
            "Focus on first panic/backtrace/stack marker rather than downstream command timeouts.",
        );
        self.logger.warn(
            "Capture/reset guidance: compare METRICS BOOT reset_code before/after run when unexpected reboot is suspected.",
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

    fn action_mark_success(&mut self) {
        self.set_success();
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
            "preflight" => {
                let _ = context;
                self.action_preflight();
                Ok(())
            }
            "flash_firmware_once" => self.action_flash_firmware_once(context),
            "run_uart_probes_once" => self.action_run_uart_probes_once(context),
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
            "mark_success" => {
                let _ = context;
                self.action_mark_success();
                Ok(())
            }
            "print_summary" => {
                self.action_print_summary();
                Ok(())
            }
            other => Err(anyhow!("unsupported troubleshoot workflow action: {other}")),
        }
    }

    fn invoke_with_result(
        &mut self,
        action: &str,
        args: &Value,
        context: &mut Value,
    ) -> Result<Option<Value>> {
        match action {
            "preflight" => {
                self.action_preflight();
                Ok(Some(self.build_preflight_result()))
            }
            "flash_firmware_once" | "run_uart_probes_once" | "run_boot_soak" => {
                self.invoke(action, args, context)?;
                Ok(Some(self.build_status_result()))
            }
            "mark_success" => {
                self.action_mark_success();
                Ok(Some(self.build_status_result()))
            }
            _ => {
                self.invoke(action, args, context)?;
                Ok(None)
            }
        }
    }
}
