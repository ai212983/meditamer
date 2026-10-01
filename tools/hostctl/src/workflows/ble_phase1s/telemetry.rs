use super::*;

use std::time::Duration;

use crate::serial_console::SerialConsole;

pub(super) const TELEMETRY_PROFILE_ENV: &str = "HOSTCTL_BLE_TELEMETRY_PROFILE";

const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
const SET_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TelemetryProfile {
    Current,
    Minimal,
}

impl TelemetryProfile {
    pub(super) fn parse(raw: Option<&str>) -> Result<Self> {
        match raw.map(str::trim).unwrap_or_default() {
            "" | "current" | "CURRENT" => Ok(TelemetryProfile::Current),
            "minimal" | "MINIMAL" => Ok(TelemetryProfile::Minimal),
            other => {
                bail!("{TELEMETRY_PROFILE_ENV} must be \"current\" or \"minimal\", got {other:?}")
            }
        }
    }

    pub(super) fn from_env() -> Result<Self> {
        Self::parse(std::env::var(TELEMETRY_PROFILE_ENV).ok().as_deref())
    }

    pub(super) fn as_str(&self) -> &'static str {
        match self {
            TelemetryProfile::Current => "current",
            TelemetryProfile::Minimal => "minimal",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct TelemetryMask {
    pub(super) mask: u32,
    pub(super) wifi: bool,
    pub(super) reassoc: bool,
    pub(super) net: bool,
    pub(super) http: bool,
    pub(super) sd: bool,
}

impl TelemetryMask {
    fn all_off(&self) -> bool {
        !self.wifi && !self.reassoc && !self.net && !self.http && !self.sd
    }

    fn required_on_domains(&self) -> Vec<&'static str> {
        let mut domains = Vec::new();
        if self.wifi {
            domains.push("WIFI");
        }
        if self.reassoc {
            domains.push("REASSOC");
        }
        if self.net {
            domains.push("NET");
        }
        if self.http {
            domains.push("HTTP");
        }
        if self.sd {
            domains.push("SD");
        }
        domains
    }
}

pub(super) fn telemetry_status_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"^TELEM mask=0x([0-9a-fA-F]+) wifi=(on|off) reassoc=(on|off) net=(on|off) http=(on|off) sd=(on|off)$",
    )?)
}

pub(super) fn telemetry_set_ok_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"^TELEMSET OK mask=0x([0-9a-fA-F]+) wifi=(on|off) reassoc=(on|off) net=(on|off) http=(on|off) sd=(on|off)$",
    )?)
}

fn parse_mask_captures(line: &str, captures: &regex::Captures<'_>) -> Result<TelemetryMask> {
    let mask = u32::from_str_radix(&captures[1], 16)
        .with_context(|| format!("invalid telemetry mask in {line}"))?;
    let on = |index: usize| -> Result<bool> {
        match &captures[index] {
            "on" => Ok(true),
            "off" => Ok(false),
            other => Err(anyhow!("invalid telemetry domain state in {line}: {other}")),
        }
    };
    let parsed = TelemetryMask {
        mask,
        wifi: on(2)?,
        reassoc: on(3)?,
        net: on(4)?,
        http: on(5)?,
        sd: on(6)?,
    };
    let domain_mask = u32::from(parsed.wifi)
        | (u32::from(parsed.reassoc) << 1)
        | (u32::from(parsed.net) << 2)
        | (u32::from(parsed.http) << 3)
        | (u32::from(parsed.sd) << 4);
    if mask != domain_mask {
        bail!("telemetry mask does not match domain states in {line}");
    }
    Ok(parsed)
}

pub(super) fn parse_telemetry_mask(line: &str, regex: &Regex) -> Result<TelemetryMask> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid correlated telemetry status: {line}"))?;
    parse_mask_captures(line, &captures)
}

pub(super) fn parse_telemetry_set_ok(line: &str, regex: &Regex) -> Result<TelemetryMask> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid correlated telemetry set response: {line}"))?;
    parse_mask_captures(line, &captures)
}

pub(super) fn query_telemetry_mask(console: &mut SerialConsole) -> Result<TelemetryMask> {
    let regex = telemetry_status_regex()?;
    let line = console
        .command_wait_regex("TELEM", &regex, QUERY_TIMEOUT)?
        .ok_or_else(|| anyhow!("missing correlated telemetry status"))?;
    parse_telemetry_mask(&line, &regex)
}

fn send_telemetry_set(console: &mut SerialConsole, args: &str) -> Result<TelemetryMask> {
    let regex = telemetry_set_ok_regex()?;
    let command = format!("TELEMSET {args}");
    let line = console
        .command_wait_regex(&command, &regex, SET_TIMEOUT)?
        .ok_or_else(|| anyhow!("missing correlated telemetry set response for {command}"))?;
    parse_telemetry_set_ok(&line, &regex)
}

// Snapshot the boot mask, then apply the selected profile. The default
// current profile never quiets telemetry; minimal selects TELEMSET NONE.
pub(super) fn snapshot_and_apply_telemetry(runtime: &mut BlePhase1sRuntime<'_>) -> Result<()> {
    let original = query_telemetry_mask(&mut runtime.console)?;
    runtime.telemetry_original = Some(original);
    match runtime.telemetry_profile {
        TelemetryProfile::Current => {
            runtime.telemetry_applied = Some(original);
            runtime.telemetry_mutated = false;
            runtime.telemetry_restored = Some(true);
        }
        TelemetryProfile::Minimal => {
            // A lost acknowledgement can follow an applied mutation. Arm
            // restoration before sending the first state-changing command.
            runtime.telemetry_mutated = true;
            runtime.telemetry_restored = Some(false);
            let cleared = send_telemetry_set(&mut runtime.console, "NONE")?;
            if !cleared.all_off() {
                bail!("minimal telemetry profile did not quiet all domains: {cleared:?}");
            }
            let applied = query_telemetry_mask(&mut runtime.console)?;
            if !applied.all_off() {
                bail!("minimal telemetry profile did not verify quiet: {applied:?}");
            }
            runtime.telemetry_applied = Some(applied);
        }
    }
    Ok(())
}

// Restore the exact boot mask with TELEMSET NONE plus one ON per originally
// enabled domain, then verify by query. A verification mismatch is a restore
// failure: the original mask stays recorded and the caller must surface the
// error rather than claim a restore.
pub(super) fn restore_telemetry_profile(runtime: &mut BlePhase1sRuntime<'_>) -> Result<()> {
    let Some(original) = runtime.telemetry_original else {
        return Ok(());
    };
    if !runtime.telemetry_mutated {
        runtime.telemetry_restored = Some(true);
        return Ok(());
    }
    send_telemetry_set(&mut runtime.console, "NONE")?;
    for domain in original.required_on_domains() {
        send_telemetry_set(&mut runtime.console, &format!("{domain} ON"))?;
    }
    let restored = query_telemetry_mask(&mut runtime.console)?;
    if restored != original {
        runtime.telemetry_restored = Some(false);
        runtime.telemetry_restore_error = Some(format!(
            "telemetry mask mismatch after restore: {restored:?}"
        ));
        bail!("telemetry restore verification failed: original={original:?} restored={restored:?}");
    }
    runtime.telemetry_restored = Some(true);
    runtime.telemetry_restore_error = None;
    Ok(())
}
