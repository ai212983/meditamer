use super::capture::{capture_boot_window, capture_stream_window};
use super::runtime_helpers::context_set_u64;
use super::FlashCaptureRuntime;
use std::fs::File;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

impl FlashCaptureRuntime<'_> {
    pub(super) fn action_capture(&mut self, args: &Value, context: &mut Value) -> Result<()> {
        let mode = args
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("capture requires mode"))?;
        match mode {
            "boot" => {
                self.logger.info(format!(
                    "capturing boot log for {} ms on {} -> {}",
                    self.boot_window.as_millis(),
                    self.port,
                    self.outputs.capture_log.display()
                ));
                let bytes = capture_boot_window(
                    &self.port,
                    self.baud,
                    self.boot_window,
                    &self.outputs.capture_log,
                )?;
                self.capture_bytes = bytes.len();
                self.logger.info(format!(
                    "boot capture complete: {} bytes -> {}",
                    self.capture_bytes,
                    self.outputs.capture_log.display()
                ));
            }
            "stream" => {
                self.logger.info(format!(
                    "capturing serial stream for {} ms on {} -> {}",
                    self.boot_window.as_millis(),
                    self.port,
                    self.outputs.capture_log.display()
                ));
                let bytes = capture_stream_window(
                    &self.port,
                    self.baud,
                    self.boot_window,
                    &self.outputs.capture_log,
                )?;
                self.capture_bytes = bytes.len();
                self.logger.info(format!(
                    "stream capture complete: {} bytes -> {}",
                    self.capture_bytes,
                    self.outputs.capture_log.display()
                ));
            }
            "none" => {
                File::create(&self.outputs.capture_log)?;
                self.capture_bytes = 0;
                self.logger.info(format!(
                    "capture skipped; created empty {}",
                    self.outputs.capture_log.display()
                ));
            }
            other => bail!("unsupported capture mode `{other}`"),
        }
        context_set_u64(context, "capture_bytes", self.capture_bytes as u64);
        Ok(())
    }
}
