use anyhow::{anyhow, Context, Result};

use crate::port_detect;

pub fn parse_env_u32(name: &str, default: u32) -> Result<u32> {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<u32>()
            .with_context(|| format!("{name} must be an unsigned integer")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("{name} invalid: {err}")),
    }
}

pub fn parse_env_u64(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<u64>()
            .with_context(|| format!("{name} must be an unsigned integer")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("{name} invalid: {err}")),
    }
}

pub fn parse_env_bool01(name: &str, default: bool) -> Result<bool> {
    match std::env::var(name) {
        Ok(raw) => match raw.as_str() {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(anyhow!("{name} must be 0 or 1")),
        },
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("{name} invalid: {err}")),
    }
}

pub fn parse_env_f64(name: &str, default: f64) -> Result<f64> {
    match std::env::var(name) {
        Ok(raw) => raw
            .parse::<f64>()
            .with_context(|| format!("{name} must be a number")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("{name} invalid: {err}")),
    }
}

pub fn require_port() -> Result<String> {
    if let Ok(port) = std::env::var("HOSTCTL_PORT") {
        if !port.trim().is_empty() {
            return Ok(port);
        }
    }

    if let Some(port) = port_detect::detect_port() {
        return Ok(port);
    }

    let candidates = port_detect::list_candidates();
    let mut message = String::from(
        "HOSTCTL_PORT is not set and autodetection was not conclusive. Set HOSTCTL_PORT explicitly.",
    );
    if !candidates.is_empty() {
        message.push_str(" Candidates:\n");
        for candidate in candidates {
            message.push_str("  - ");
            message.push_str(&candidate);
            message.push('\n');
        }
    }
    Err(anyhow!(message))
}

pub fn baud_from_env(default: u32) -> Result<u32> {
    parse_env_u32("HOSTCTL_BAUD", default)
}
