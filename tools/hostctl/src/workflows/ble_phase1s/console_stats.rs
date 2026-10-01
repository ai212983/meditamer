use super::*;

use std::time::Duration;

use crate::serial_console::SerialConsole;

pub(super) const STAGE_BASELINE: &str = "baseline";
pub(super) const STAGE_BEFORE_UPLOAD_BEFORE: &str = "before_upload/before";
pub(super) const STAGE_AFTER_UPLOAD_BEFORE: &str = "after_upload/before";
pub(super) const STAGE_BEFORE_UPLOAD_AFTER: &str = "before_upload/after";
pub(super) const STAGE_OFF_CONFIRMED: &str = "off_confirmed";
pub(super) const STAGE_BLE_COMPLETED: &str = "ble_completed";
pub(super) const STAGE_RESTORED: &str = "restored";
pub(super) const STAGE_AFTER_UPLOAD_AFTER: &str = "after_upload/after";
pub(super) const STAGE_FINAL: &str = "final";

const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_QUERY_ATTEMPTS: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct ConsoleDropSnapshot {
    pub(super) total: u32,
    pub(super) contention: u32,
    pub(super) deferred_overflow: u32,
    pub(super) deferred_oversize: u32,
    pub(super) stable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ConsoleDropSample {
    pub(super) cycle: u32,
    pub(super) stage: &'static str,
    pub(super) boot: Option<u32>,
    pub(super) total: u32,
    pub(super) contention: u32,
    pub(super) deferred_overflow: u32,
    pub(super) deferred_oversize: u32,
    pub(super) stable: bool,
}

impl ConsoleDropSample {
    fn snapshot(&self) -> ConsoleDropSnapshot {
        ConsoleDropSnapshot {
            total: self.total,
            contention: self.contention,
            deferred_overflow: self.deferred_overflow,
            deferred_oversize: self.deferred_oversize,
            stable: self.stable,
        }
    }
}

pub(super) fn console_stats_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"^CONSOLE_DROPS total=([0-9]+) contention=([0-9]+) deferred_overflow=([0-9]+) deferred_oversize=([0-9]+) stable=(true|false)$",
    )?)
}

pub(super) fn parse_console_stats(line: &str, regex: &Regex) -> Result<ConsoleDropSnapshot> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid correlated console drop status: {line}"))?;
    let number = |index: usize| -> Result<u32> {
        captures[index]
            .parse::<u32>()
            .with_context(|| format!("invalid console drop number in {line}"))
    };
    let snapshot = ConsoleDropSnapshot {
        total: number(1)?,
        contention: number(2)?,
        deferred_overflow: number(3)?,
        deferred_oversize: number(4)?,
        stable: &captures[5] == "true",
    };
    if snapshot.stable
        && snapshot.total
            != snapshot
                .contention
                .wrapping_add(snapshot.deferred_overflow)
                .wrapping_add(snapshot.deferred_oversize)
    {
        bail!("stable console drop snapshot has inconsistent cause accounting");
    }
    Ok(snapshot)
}

// A stable=false snapshot can tear across counter updates, so retry the
// correlated query a bounded number of times. The counters are read-only
// host-side; no clear command is ever issued.
pub(super) fn query_console_stats(console: &mut SerialConsole) -> Result<ConsoleDropSnapshot> {
    let regex = console_stats_regex()?;
    let mut last = None;
    for _ in 0..MAX_QUERY_ATTEMPTS {
        let line = console
            .command_wait_regex("CONSOLESTATS", &regex, QUERY_TIMEOUT)?
            .ok_or_else(|| anyhow!("missing correlated console drop status"))?;
        let snapshot = parse_console_stats(&line, &regex)?;
        if snapshot.stable {
            return Ok(snapshot);
        }
        last = Some(snapshot);
    }
    Err(anyhow!(
        "console drop counters did not stabilize within {MAX_QUERY_ATTEMPTS} queries: {last:?}"
    ))
}

pub(super) fn sample_console_stage(
    runtime: &mut BlePhase1sRuntime<'_>,
    cycle: u32,
    stage: &'static str,
) -> Result<()> {
    let snapshot = query_console_stats(&mut runtime.console)?;
    runtime.console_drop_samples.push(ConsoleDropSample {
        cycle,
        stage,
        boot: runtime.boot_generation,
        total: snapshot.total,
        contention: snapshot.contention,
        deferred_overflow: snapshot.deferred_overflow,
        deferred_oversize: snapshot.deferred_oversize,
        stable: snapshot.stable,
    });
    Ok(())
}

// Cumulative counters must never decrease across stage samples. A decrease
// means the device restarted without a boot-generation change being observed
// yet, or the counter path itself is incoherent; either way the gate cannot
// trust the accumulated evidence.
pub(super) fn console_drop_violations(samples: &[ConsoleDropSample]) -> Vec<String> {
    let mut violations = Vec::new();
    for pair in samples.windows(2) {
        let (previous, current) = (&pair[0], &pair[1]);
        let mut regressed = Vec::new();
        for (label, before, after) in [
            ("total", previous.total, current.total),
            ("contention", previous.contention, current.contention),
            (
                "deferred_overflow",
                previous.deferred_overflow,
                current.deferred_overflow,
            ),
            (
                "deferred_oversize",
                previous.deferred_oversize,
                current.deferred_oversize,
            ),
        ] {
            if after < before {
                regressed.push(format!("{label} {before}->{after}"));
            }
        }
        if regressed.is_empty() {
            continue;
        }
        let reset = regressed.len() == 4;
        violations.push(format!(
            "console drop counter regressed at cycle={} stage={}: {}{}",
            current.cycle,
            current.stage,
            regressed.join(", "),
            if reset {
                " (possible device reset)"
            } else {
                ""
            },
        ));
    }
    violations
}

pub(super) fn console_drop_aggregate(
    samples: &[ConsoleDropSample],
) -> (
    Option<ConsoleDropSnapshot>,
    Option<ConsoleDropSnapshot>,
    Option<u32>,
) {
    let baseline = samples
        .iter()
        .find(|sample| sample.stage == STAGE_BASELINE)
        .map(ConsoleDropSample::snapshot);
    let final_sample = samples.last().map(ConsoleDropSample::snapshot);
    let during_gate = match (baseline, final_sample) {
        (Some(baseline), Some(final_sample)) => final_sample.total.checked_sub(baseline.total),
        _ => None,
    };
    (baseline, final_sample, during_gate)
}
