impl WorkflowRuntime for FlashCaptureRuntime<'_> {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        match action {
            "preflight" => self.action_preflight(context),
            "resolve_image" => self.action_resolve_image(args),
            "archive_image" => self.action_archive_image(),
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
