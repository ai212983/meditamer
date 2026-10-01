use super::Mode;
use anyhow::{anyhow, bail, ensure, Result};
use serde_json::{json, Value};

pub(super) fn field<'a>(line: &'a str, key: &str) -> Result<&'a str> {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(key).and_then(|s| s.strip_prefix('=')))
        .ok_or_else(|| anyhow!("missing {key}: {line}"))
}
pub(super) fn number(line: &str, key: &str) -> Result<u64> {
    Ok(field(line, key)?.parse()?)
}
pub(super) fn sample_number(line: &str, key: &str) -> Result<u64> {
    Ok(field(line, key)?
        .strip_prefix("Some(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| anyhow!("missing successful {key}"))?
        .parse()?)
}
pub(super) fn unique<'a>(lines: &'a [String], prefix: &str, id: u64) -> Result<(usize, &'a str)> {
    let matching: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with(prefix) && number(l, "id").ok() == Some(id))
        .collect();
    ensure!(
        matching.len() == 1,
        "expected one {prefix} for {id}, got {}",
        matching.len()
    );
    Ok((matching[0].0, matching[0].1))
}

pub(super) fn transition(lines: &[String], id: u64, mode: Mode) -> Result<usize> {
    let (begin, start) = unique(lines, "OBSSLEEP BEGIN ", id)?;
    ensure!(field(start, "mode")? == mode.wire(), "wrong sleep mode");
    let (control, ack) = unique(lines, "OBSSLEEP CONTROL ", id)?;
    let deep = mode == Mode::Deep;
    let (end, result) = unique(
        lines,
        if deep {
            "OBSSLEEP ENTER "
        } else {
            "OBSSLEEP END "
        },
        id,
    )?;
    ensure!(
        begin < control && control < end,
        "sleep control order is invalid"
    );
    ensure!(
        number(result, "at_ms")? < number(start, "expires_at_ms")?,
        "expired sleep operation"
    );
    for (name, id_key, ack_key, failed) in [
        (
            "ENVIRONMENT",
            "environment_id",
            "environment_ack",
            mode == Mode::RejectEnvironment,
        ),
        (
            "BATTERY",
            "battery_id",
            "battery_ack",
            mode == Mode::RejectBattery,
        ),
    ] {
        let control_id = number(ack, id_key)?;
        ensure!(
            control_id > 0 && control_id < u64::from(u32::MAX),
            "invalid control identity"
        );
        let outcome = if failed { "CleanupFailed" } else { "Quiesced" };
        ensure!(
            field(ack, ack_key)? == outcome,
            "unexpected {name} cleanup outcome"
        );
        let expected=format!("OBSERVATION_SUSPEND_ACK provider={name} id={control_id} outcome={outcome} accepted=true");
        ensure!(
            lines[begin + 1..control]
                .iter()
                .filter(|l| **l == expected)
                .count()
                == 1,
            "missing current {name} acknowledgement"
        );
        let injected = format!("OBSERVATION_CLEANUP_INJECTED provider={name} id={control_id}");
        ensure!(
            lines[begin + 1..control]
                .iter()
                .filter(|l| **l == injected)
                .count()
                == usize::from(failed),
            "unexpected injection count for {name}"
        );
        if !deep {
            ensure!(
                number(result, id_key)? > control_id,
                "missing newer resume for {name}"
            );
        }
    }
    if !deep {
        ensure!(
            field(result, "status")? == "Rejected",
            "sleep was not rejected"
        );
    }
    Ok(end)
}

pub(super) fn validate(lines: &[String], id: u64, mode: Mode) -> Result<Option<Value>> {
    let end = transition(lines, id, mode)?;
    let (begin, _) = unique(lines, "OBSSLEEP BEGIN ", id)?;
    for line in &lines[begin..] {
        if line.contains("panic")
            || line.contains("WATCHDOG")
            || line.contains("_ACQUIRE_FAILED")
            || line.contains("SHTC3_SUSPEND_FAILED")
            || (line.starts_with("BUTTON_EVENT ") && line.contains("kind=pressed"))
        {
            bail!("unexpected failure or operator input: {line}");
        }
    }
    let resets: Vec<_> = lines[begin..]
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("RESET "))
        .collect();
    let (after, cutoff) = if mode == Mode::Deep {
        ensure!(resets.len() <= 1, "multiple resets during sleep");
        let Some((reset, line)) = resets.first() else {
            return Ok(None);
        };
        ensure!(
            begin + reset > end
                && matches!(
                    line.as_str(),
                    "RESET reason=Some(CoreDeepSleep) wake=Timer"
                        | "RESET reason=Some(CoreDeepSleep) wake=WakeupReason(EnumSet(Timer))"
                ),
            "unexpected reset or wake source"
        );
        ensure!(
            lines[end + 1..begin + reset]
                .iter()
                .filter(|line| line.as_str()
                    == "DEEP_SLEEP_ENTER wake=timer-only duration_s=15 destination=home")
                .count()
                == 1,
            "missing actual deep-sleep entry"
        );
        ensure!(
            !lines[begin..]
                .iter()
                .any(|l| l.starts_with("OBSSLEEP END ")),
            "unexpected terminal rejection"
        );
        if !lines[begin + reset..]
            .iter()
            .any(|l| l == "RUNTIME_READY app_state=ready display=ready")
        {
            return Ok(None);
        }
        (begin + reset, 0)
    } else {
        ensure!(
            resets.is_empty()
                && !lines[begin..]
                    .iter()
                    .any(|l| l.starts_with("DEEP_SLEEP_ENTER") || l.starts_with("OBSSLEEP ENTER ")),
            "rejected sleep entered sleep or reset"
        );
        (end, number(&lines[end], "at_ms")?)
    };
    let mut samples = serde_json::Map::new();
    for (provider, prefix) in [
        ("ENVIRONMENT", "HOME_SAMPLE "),
        ("BATTERY", "HOME_BATTERY "),
    ] {
        let baseline = lines[begin..end].iter().find(|l| {
            l.starts_with("OBSSLEEP BASE ")
                && number(l, "id").ok() == Some(id)
                && field(l, "provider").ok() == Some(provider)
        });
        let mut found = None;
        for line in lines[after..].iter().filter(|l| l.starts_with(prefix)) {
            let sampled = sample_number(line, "last_sample_at_ms")?;
            let revision = number(line, "revision")?;
            let generation = number(line, "generation")?;
            ensure!(
                sample_number(line, "last_sample_revision")? == revision,
                "cached or failed attempt delivered"
            );
            if provider == "BATTERY" {
                ensure!(field(line, "health")? == "Ok", "battery is not healthy");
            }
            if sampled <= cutoff {
                continue;
            }
            if mode == Mode::Deep {
                ensure!(
                    generation == 0 && revision > 0,
                    "unexpected post-reset provider identity"
                );
            } else {
                let baseline =
                    baseline.ok_or_else(|| anyhow!("missing pre-sleep {provider} baseline"))?;
                ensure!(
                    generation == number(baseline, "generation")?,
                    "provider restarted after rejection"
                );
                if revision <= number(baseline, "revision")?
                    || sampled <= sample_number(baseline, "sampled_at_ms")?
                {
                    continue;
                }
            }
            found =
                Some(json!({"generation":generation,"revision":revision,"sampled_at_ms":sampled}));
            break;
        }
        let Some(sample) = found else {
            return Ok(None);
        };
        samples.insert(provider.to_owned(), sample);
    }
    Ok(Some(
        json!({"transition_line":end,"wake":if mode==Mode::Deep {"Timer"} else {"no_sleep"},"samples":samples}),
    ))
}

#[cfg(test)]
mod tests;
