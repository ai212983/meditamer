use std::process::Command;

use anyhow::{Context, Result};

use crate::workflows::common::repo_root;

use super::{
    probes::run_uart_probe_sequence,
    utils::{format_command_output, recent_uart_lines},
    TroubleshootRuntime,
};

impl TroubleshootRuntime<'_> {
    pub(super) fn action_flash_firmware_once(&mut self) -> Result<()> {
        if !self.config.flash_first {
            self.logger
                .info("Skipping flash step (HOSTCTL_TROUBLESHOOT_FLASH_FIRST=0)");
            self.flash_ok = true;
            return Ok(());
        }

        self.close_console();

        let script = repo_root().join("scripts/device/flash.sh");
        let repo_dir = repo_root();
        self.logger.info("Flash attempt...");
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
                self.logger.info("Flash step: PASS");
                Ok(())
            }
            Ok(output) => {
                let detail = format!(
                    "flash.sh exited with status {}\n{}",
                    output.status,
                    format_command_output(&output)
                );
                self.flash_ok = false;
                Err(self.failure_action_error("flash", detail.clone()).into())
            }
            Err(err) => {
                let detail = format!("failed to execute flash script: {err:#}");
                self.flash_ok = false;
                Err(self.failure_action_error("flash", detail.clone()).into())
            }
        }
    }

    pub(super) fn action_run_uart_probes_once(&mut self) -> Result<()> {
        self.probe_ok = false;
        let retries = 1;
        let delay_ms = self.config.probe_delay_ms;
        let timeout_ms = self.config.probe_timeout_ms;

        let console = match self.ensure_console() {
            Ok(console) => console,
            Err(err) => {
                return Err(self
                    .failure_action_error("probe", format!("failed to open serial: {err:#}"))
                    .into());
            }
        };

        let probe_result = run_uart_probe_sequence(console, retries, delay_ms, timeout_ms);

        match probe_result {
            Ok(()) => {
                self.probe_ok = true;
                self.logger.info("UART probe step: PASS");
                Ok(())
            }
            Err(err) => {
                let detail = format!(
                    "UART probes failed: {err:#}\nRecent UART lines:\n{}",
                    recent_uart_lines(console, 20)
                );
                Err(self.failure_action_error("probe", detail.clone()).into())
            }
        }
    }

    pub(super) fn action_run_boot_soak(&mut self) -> Result<()> {
        self.soak_ok = false;
        self.close_console();

        crate::logging::ensure_parent_dir(&self.soak_log_dir.join("placeholder"))?;

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
                self.logger.info("Boot soak step: PASS");
                Ok(())
            }
            Ok(output) => {
                let detail = format!(
                    "soak_boot.sh exited with status {}\n{}",
                    output.status,
                    format_command_output(&output)
                );
                self.record_failure("soak", detail);
                Ok(())
            }
            Err(err) => {
                self.record_failure(
                    "soak",
                    format!("failed to execute soak boot script: {err:#}"),
                );
                Ok(())
            }
        }
    }
}
