use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde_json::Value;

pub(crate) fn resolve_templates(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let remaining = &rest[start + 1..];
        let end = remaining
            .find('}')
            .ok_or_else(|| anyhow!("unclosed template placeholder in '{template}'"))?;
        let key = remaining[..end].trim();
        if key.is_empty() {
            return Err(anyhow!("empty template placeholder in '{template}'"));
        }
        let replacement = vars
            .get(key)
            .ok_or_else(|| anyhow!("unknown template variable '{{{key}}}'"))?;
        output.push_str(replacement);
        rest = &remaining[end + 1..];
    }

    output.push_str(rest);
    Ok(output)
}

pub(super) fn required_arg_str<'a>(args: &'a Value, key: &str, action: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("{action} requires string argument '{key}'"))
}

pub(super) fn optional_arg_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(|value| value.as_u64())
        .and_then(|raw| u32::try_from(raw).ok())
}
