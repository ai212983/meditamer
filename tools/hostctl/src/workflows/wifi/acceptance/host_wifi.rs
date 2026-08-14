use std::process::Command;

use anyhow::{anyhow, Context, Result};

#[cfg(target_os = "macos")]
pub(crate) fn ensure_host_wifi_association(expected_ssid: &str) -> Result<()> {
    let ports_out = Command::new("networksetup")
        .arg("-listallhardwareports")
        .output()
        .context("failed to execute networksetup -listallhardwareports")?;
    if !ports_out.status.success() {
        return Err(anyhow!(
            "networksetup -listallhardwareports failed: {}",
            String::from_utf8_lossy(&ports_out.stderr)
        ));
    }
    let ports_text = String::from_utf8_lossy(&ports_out.stdout);
    let mut wifi_device: Option<String> = None;
    let mut saw_wifi_port = false;
    for line in ports_text.lines() {
        if line.starts_with("Hardware Port: ") {
            saw_wifi_port = line.trim() == "Hardware Port: Wi-Fi";
            continue;
        }
        if saw_wifi_port && line.starts_with("Device: ") {
            wifi_device = Some(line["Device: ".len()..].trim().to_string());
            break;
        }
    }
    let Some(device) = wifi_device else {
        return Err(anyhow!(
            "unable to determine host Wi-Fi interface via networksetup"
        ));
    };

    let assoc_out = Command::new("networksetup")
        .args(["-getairportnetwork", &device])
        .output()
        .with_context(|| format!("failed to execute networksetup -getairportnetwork {device}"))?;
    if !assoc_out.status.success() {
        return Ok(());
    }
    let assoc_text = String::from_utf8_lossy(&assoc_out.stdout)
        .trim()
        .to_string();
    let assoc_lower = assoc_text.to_ascii_lowercase();
    if assoc_lower.contains("not associated") {
        return Ok(());
    }
    let current_ssid = assoc_text
        .split(':')
        .next_back()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    if current_ssid != expected_ssid {
        return Err(anyhow!(
            "host Wi-Fi associated to SSID {} on {}, expected {}",
            current_ssid,
            device,
            expected_ssid
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn ensure_host_wifi_association(_expected_ssid: &str) -> Result<()> {
    Ok(())
}
