//! Active periodic leases must close within this rejection and restore current
//! Home demand. An entry refresh alone cannot establish periodic recovery.
use super::{
    evidence::{field, number, sample_number, unique},
    Mode,
};
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};

// Current Medinote Home freshness contract, not a predicted latency margin.
const HOME_AGE_MS: u64 = 60_000;

#[derive(Clone, Copy, Debug)]
pub(super) struct Setup {
    pub interval_ms: u32,
    pub validity_ms: u32,
}

impl Setup {
    pub fn new(interval_ms: u32, validity_ms: u32, id: u64, mode: Mode) -> Result<Self> {
        ensure!(
            mode != Mode::Deep,
            "active periodic restoration requires a rejection mode"
        );
        ensure!(
            id > 2,
            "two preceding nonzero IDs are reserved for periodic fixtures"
        );
        ensure!(
            (60_001..=300_000).contains(&interval_ms),
            "period must exceed Home's 60000ms and be at most 300000ms"
        );
        ensure!(
            validity_ms > interval_ms && validity_ms <= 900_000,
            "periodic validity must exceed period and be at most 900000ms"
        );
        Ok(Self {
            interval_ms,
            validity_ms,
        })
    }
    pub fn command(self, sleep_id: u64, provider: u64) -> Result<String> {
        let name = match provider {
            1 => "SHTC3",
            2 => "ADC",
            _ => bail!("invalid Medinote provider"),
        };
        Ok(format!(
            "OBSPER {name} {} {} {}",
            fixture_id(sleep_id, provider),
            self.interval_ms,
            self.validity_ms
        ))
    }
    pub fn applied(self, lines: &[String], sleep_id: u64, provider: u64) -> Result<bool> {
        let id = fixture_id(sleep_id, provider);
        for line in lines {
            if line.starts_with("RESET ")
                || line.contains("_ACQUIRE_FAILED")
                || line.contains("panic")
                || line.contains("WATCHDOG")
                || (line.starts_with("BUTTON_EVENT ") && line.contains("kind=pressed"))
            {
                bail!("unexpected event while arming periodic fixtures: {line}");
            }
            if line.starts_with("OBSPER RESULT ")
                && [sleep_id - 2, sleep_id - 1].contains(&number(line, "id")?)
            {
                bail!("periodic fixture ended before sleep: {line}");
            }
        }
        if !lines
            .iter()
            .any(|l| l.starts_with("OBSPER APPLIED ") && number(l, "id").ok() == Some(id))
        {
            return Ok(false);
        }
        let (_, applied) = unique(lines, "OBSPER APPLIED ", id)?;
        ensure!(
            number(applied, "provider")? == provider && field(applied, "status")? == "Applied",
            "wrong periodic application"
        );
        ensure!(
            number(applied, "interval_ms")? == u64::from(self.interval_ms),
            "wrong periodic interval"
        );
        let at = number(applied, "at_ms")?;
        ensure!(
            number(applied, "applied_at_ms")? == at && number(applied, "expires_at_ms")? > at,
            "invalid periodic application time"
        );
        Ok(true)
    }
    pub fn validate(
        self,
        lines: &[String],
        sleep_id: u64,
        recovery: &Value,
    ) -> Result<Option<Value>> {
        let (begin, _) = unique(lines, "OBSSLEEP BEGIN ", sleep_id)?;
        let (end, _) = unique(lines, "OBSSLEEP END ", sleep_id)?;
        let mut providers = serde_json::Map::new();
        for (provider, name, ack_key, prefix) in [
            (1, "ENVIRONMENT", "environment_id", "HOME_SAMPLE "),
            (2, "BATTERY", "battery_id", "HOME_BATTERY "),
        ] {
            ensure!(
                self.applied(&lines[..begin], sleep_id, provider)?,
                "periodic fixture was not applied before sleep"
            );
            let (closed_at, expiry) = closure_info(lines, sleep_id, provider, name, ack_key)?;
            if !restored_demand(&lines[end + 1..], sleep_id, provider)? {
                return Ok(None);
            }
            let Some(next) = later_sample(
                &lines[end + 1..],
                prefix,
                &recovery["samples"][name],
                self.interval_ms,
                expiry,
            )?
            else {
                return Ok(None);
            };
            providers.insert(name.to_owned(), json!({"fixture_id":fixture_id(sleep_id,provider),"closed_at_ms":closed_at,"live_age_ms":HOME_AGE_MS,"periodic_sample":next}));
        }
        Ok(Some(
            json!({"interval_ms":self.interval_ms,"providers":providers}),
        ))
    }
}

fn fixture_id(sleep_id: u64, provider: u64) -> u64 {
    sleep_id - 3 + provider
}

fn later_sample(
    lines: &[String],
    prefix: &str,
    first: &Value,
    interval_ms: u32,
    expiry: u64,
) -> Result<Option<Value>> {
    let generation = first["generation"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing recovery generation"))?;
    let revision = first["revision"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing recovery revision"))?;
    let sampled = first["sampled_at_ms"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("missing recovery timestamp"))?;
    for line in lines.iter().filter(|line| line.starts_with(prefix)) {
        ensure!(
            number(line, "generation")? == generation,
            "provider restarted after rejection"
        );
        let next_revision = number(line, "revision")?;
        if next_revision <= revision {
            continue;
        }
        ensure!(
            sample_number(line, "last_sample_revision")? == next_revision,
            "failed attempt masquerades as periodic recovery"
        );
        if prefix == "HOME_BATTERY " {
            ensure!(
                field(line, "health")? == "Ok",
                "periodic battery recovery is unhealthy"
            );
        }
        let next = sample_number(line, "last_sample_at_ms")?;
        let elapsed = next
            .checked_sub(sampled)
            .ok_or_else(|| anyhow::anyhow!("successful sample time moved backwards"))?;
        if elapsed < HOME_AGE_MS {
            continue;
        }
        ensure!(
            elapsed < u64::from(interval_ms) && next < expiry,
            "recovery did not distinguish live cadence from old override or expiry"
        );
        return Ok(Some(
            json!({"generation":generation,"revision":next_revision,"sampled_at_ms":next,"interval_ms":elapsed}),
        ));
    }
    Ok(None)
}

fn closure_info(
    lines: &[String],
    sleep_id: u64,
    provider: u64,
    name: &str,
    ack_key: &str,
) -> Result<(u64, u64)> {
    let (begin, start) = unique(lines, "OBSSLEEP BEGIN ", sleep_id)?;
    let (end, finish) = unique(lines, "OBSSLEEP END ", sleep_id)?;
    let (_, control) = unique(lines, "OBSSLEEP CONTROL ", sleep_id)?;
    let id = fixture_id(sleep_id, provider);
    let (_, applied) = unique(lines, "OBSPER APPLIED ", id)?;
    let (closed_at, closed) = unique(lines, "OBSPER RESULT ", id)?;
    let ack = format!(
        "OBSERVATION_SUSPEND_ACK provider={name} id={} ",
        number(control, ack_key)?
    );
    let ack_at = lines
        .iter()
        .position(|line| line.starts_with(&ack))
        .ok_or_else(|| anyhow::anyhow!("missing cleanup acknowledgement"))?;
    ensure!(
        begin < closed_at && closed_at < ack_at && ack_at < end,
        "fixture closure is outside this sleep cleanup"
    );
    ensure!(
        number(closed, "provider")? == provider && field(closed, "status")? == "Closed",
        "fixture did not close on control"
    );
    for key in ["applied_at_ms", "expires_at_ms", "interval_ms"] {
        ensure!(
            number(closed, key)? == number(applied, key)?,
            "mismatched periodic {key}"
        );
    }
    ensure!(
        number(closed, "at_ms")? >= number(start, "at_ms")?
            && number(closed, "at_ms")? <= number(finish, "at_ms")?
            && number(closed, "at_ms")? < number(closed, "expires_at_ms")?,
        "fixture closure is expired or outside the sleep window"
    );
    ensure!(
        number(closed, "live_fields")? == 0
            && field(closed, "live_age0_ms")? == "none"
            && field(closed, "live_age1_ms")? == "none",
        "Home demand was not withdrawn for sleep"
    );
    ensure!(
        !lines[closed_at + 1..]
            .iter()
            .any(|l| l.starts_with("OBSPER SAMPLE ") && number(l, "id").ok() == Some(id)),
        "closed override emitted a later sample"
    );
    Ok((number(closed, "at_ms")?, number(closed, "expires_at_ms")?))
}

fn restored_demand(lines: &[String], sleep_id: u64, provider: u64) -> Result<bool> {
    let snapshots: Vec<_> = lines
        .iter()
        .filter(|line| {
            line.starts_with("OBSSLEEP DEMAND ")
                && number(line, "id").ok() == Some(sleep_id)
                && number(line, "provider").ok() == Some(provider)
        })
        .collect();
    if snapshots.is_empty() {
        return Ok(false);
    }
    ensure!(snapshots.len() == 1, "duplicate restored demand snapshot");
    let snapshot = snapshots[0];
    ensure!(
        field(snapshot, "pending")? == "false" && number(snapshot, "live_fields")? == 3,
        "override remains or Home fields were not restored"
    );
    ensure!(
        number(snapshot, "live_age0_ms")? == HOME_AGE_MS
            && number(snapshot, "live_age1_ms")? == HOME_AGE_MS,
        "current Home cadence was not restored"
    );
    Ok(true)
}
