use super::runtime_helpers::{action_abort_flash, action_write_summary};
use super::FlashCaptureRuntime;

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::scenarios::WorkflowRuntime;

impl WorkflowRuntime for FlashCaptureRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "preflight" => self.action_preflight(context),
            "resolve_image" => self.action_resolve_image(args),
            "prepare_bootloader" => self.action_prepare_bootloader(),
            "archive_image" => self.action_archive_image(args),
            "prepare_idf_env" => self.action_prepare_idf_env(),
            "flash" => self.action_flash(args),
            "capture" => self.action_capture(args, context),
            "post_command" => self.action_post_command(context),
            "write_summary" => action_write_summary(self, context),
            "abort_flash" => action_abort_flash(context),
            other => Err(anyhow!(
                "unsupported flash-capture workflow action: {other}"
            )),
        }
    }
}
