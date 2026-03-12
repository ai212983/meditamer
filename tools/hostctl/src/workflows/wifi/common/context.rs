use anyhow::{anyhow, Result};
use serde_json::Value;

pub fn ctx_get_u32(context: &Value, key: &str) -> Result<u32> {
    context
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| anyhow!("missing context key `{key}` as u32"))
}

pub fn ctx_get_string(context: &Value, key: &str) -> Result<String> {
    context
        .get(key)
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .ok_or_else(|| anyhow!("missing context key `{key}` as string"))
}

pub fn ctx_set_u32(context: &mut Value, key: &str, value: u32) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

pub fn ctx_set_bool(context: &mut Value, key: &str, value: bool) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}

pub fn ctx_set_string(context: &mut Value, key: &str, value: &str) -> Result<()> {
    let map = context
        .as_object_mut()
        .ok_or_else(|| anyhow!("workflow context is not an object"))?;
    map.insert(key.to_string(), Value::from(value));
    Ok(())
}
