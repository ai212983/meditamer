use anyhow::{anyhow, Result};
use serde_json::Value;

pub(super) fn ctx_set_bool(context: &mut Value, key: &str, value: bool) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

pub(super) fn ctx_set_string(context: &mut Value, key: &str, value: &str) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

pub(super) fn ctx_set_u32(context: &mut Value, key: &str, value: u32) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}
