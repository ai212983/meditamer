fn action_write_summary(runtime: &mut FlashCaptureRuntime<'_>, context: &mut Value) -> Result<()> {
    let result = runtime
        .flash_result
        .as_ref()
        .ok_or_else(|| anyhow!("cannot write summary without flash result"))?;
    write_summary(
        &runtime.outputs,
        &runtime.port,
        runtime.baud,
        runtime.flash_baud,
        result,
        runtime.opts.capture_mode,
        runtime.capture_bytes,
    )?;
    context_set_string(
        context,
        "artifact_root",
        &runtime.outputs.root.display().to_string(),
    );
    Ok(())
}

fn action_abort_flash(context: &Value) -> Result<()> {
    let detail = context
        .get("flash_error")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("flash failed");
    Err(anyhow!("{detail}"))
}

fn context_set_string(context: &mut Value, key: &str, value: &str) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn context_set_u64(context: &mut Value, key: &str, value: u64) {
    if let Some(map) = context.as_object_mut() {
        map.insert(key.to_string(), Value::Number(value.into()));
    }
}
