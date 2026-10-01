//! Periodic fixture evidence and bounded serial primitives. YAML owns whether
//! to wait for expiry or send one correlated cancellation after enough samples.
use super::{wire, FixtureProvider, ObservationFixtureRuntime};
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub(super) struct Event {
    pub(super) status: String,
    pub(super) applied_at_ms: Option<u64>,
    pub(super) at_ms: u64,
    pub(super) expires_at_ms: u64,
    pub(super) interval_ms: u32,
    pub(super) live_fields: u64,
    pub(super) live_age0_ms: Option<u64>,
    pub(super) live_age1_ms: Option<u64>,
}
#[derive(Debug, Serialize)]
pub(super) struct Sample {
    generation: u64,
    revision: u64,
    sampled_at_ms: u64,
}
#[derive(Debug, Serialize)]
pub(super) struct Evidence {
    applied: Event,
    samples: Vec<Sample>,
    restored: Event,
}

pub(super) fn event(fields: &wire::Fields<'_>) -> Result<Event> {
    Ok(Event {
        status: wire::field(fields, "status")?.into(),
        applied_at_ms: wire::optional_number(fields, "applied_at_ms")?,
        at_ms: wire::number(fields, "at_ms")?,
        expires_at_ms: wire::number(fields, "expires_at_ms")?,
        interval_ms: wire::number(fields, "interval_ms")?.try_into()?,
        live_fields: wire::number(fields, "live_fields")?,
        live_age0_ms: wire::optional_number(fields, "live_age0_ms")?,
        live_age1_ms: wire::optional_number(fields, "live_age1_ms")?,
    })
}
fn sample(fields: &wire::Fields<'_>, provider: FixtureProvider) -> Result<Sample> {
    if wire::number(fields, "fields")? != provider.fields()
        || !matches!(wire::field(fields, "health")?, "Ok" | "Degraded")
    {
        bail!("invalid periodic sample fields/health");
    }
    match provider {
        FixtureProvider::Battery | FixtureProvider::Adc => {
            if wire::number(fields, "percent")? > 100 {
                bail!("invalid periodic battery percent");
            }
            if matches!(provider, FixtureProvider::Adc) {
                let _: u32 = wire::number(fields, "millivolts")?.try_into()?;
            }
        }
        FixtureProvider::Bme688 => {
            let _: i32 = wire::field(fields, "temperature_centidegrees")?.parse()?;
            let _: u32 = wire::field(fields, "humidity_millipercent")?.parse()?;
        }
        FixtureProvider::Shtc3 => {
            let _: i32 = wire::field(fields, "temperature_millicelsius")?.parse()?;
            let _: i32 = wire::field(fields, "humidity_millipercent")?.parse()?;
        }
    }
    Ok(Sample {
        generation: wire::number(fields, "generation")?,
        revision: wire::number(fields, "revision")?,
        sampled_at_ms: wire::number(fields, "sampled_at_ms")?,
    })
}

pub(super) fn verify(
    lines: &[String],
    id: u64,
    provider: FixtureProvider,
    interval: u32,
    validity: u32,
    cancelled: bool,
) -> Result<Evidence> {
    let mut applied = None;
    let mut restored = None;
    let mut samples = Vec::new();
    let pattern = Regex::new(&format!(r"^OBSPER (APPLIED|SAMPLE|RESULT) id={id}(?: |$)"))?;
    for line in lines.iter().filter(|line| pattern.is_match(line)) {
        let mut tokens = line.splitn(3, ' ');
        tokens.next();
        let kind = tokens.next().unwrap();
        let fields = wire::parse_fields(tokens.next().unwrap())?;
        if wire::number(&fields, "provider")? != provider.id() {
            bail!("periodic provider mismatch");
        }
        match kind {
            "APPLIED" => {
                if applied.is_some() || restored.is_some() {
                    bail!("duplicate/out-of-order periodic application");
                }
                applied = Some(event(&fields)?);
            }
            "SAMPLE" => {
                if applied.is_none() || restored.is_some() {
                    bail!("periodic sample outside active session");
                }
                samples.push(sample(&fields, provider)?);
            }
            "RESULT" => {
                let status = wire::field(&fields, "status")?;
                if !matches!(status, "Restored" | "Cancelled") {
                    bail!("periodic fixture terminated with {status}");
                }
                if restored.is_some() {
                    bail!("duplicate periodic result");
                }
                restored = Some(event(&fields)?);
            }
            _ => unreachable!(),
        }
    }
    let applied = applied.ok_or_else(|| anyhow!("missing periodic application"))?;
    let restored = restored.ok_or_else(|| anyhow!("missing periodic restoration"))?;
    verify_events(&applied, &restored, interval, validity, cancelled)?;
    verify_samples(&samples, &applied, &restored, interval)?;
    Ok(Evidence {
        applied,
        samples,
        restored,
    })
}
fn verify_samples(
    samples: &[Sample],
    applied: &Event,
    restored: &Event,
    interval: u32,
) -> Result<()> {
    if samples.len() < 2 {
        bail!("periodic fixture needs at least two successful acquisitions");
    }
    for (index, sample) in samples.iter().enumerate() {
        if sample.revision == 0
            || sample.sampled_at_ms < applied.at_ms
            || sample.sampled_at_ms >= applied.expires_at_ms
            || sample.sampled_at_ms > restored.at_ms
        {
            bail!("periodic sample outside eligibility");
        }
        if index > 0 {
            let previous = &samples[index - 1];
            if sample.generation != previous.generation
                || sample.revision <= previous.revision
                || sample
                    .sampled_at_ms
                    .checked_sub(previous.sampled_at_ms)
                    .is_none_or(|delta| delta < u64::from(interval))
            {
                bail!("periodic sample identity/cadence mismatch");
            }
        }
    }
    Ok(())
}
fn verify_events(
    applied: &Event,
    restored: &Event,
    interval: u32,
    validity: u32,
    cancelled: bool,
) -> Result<()> {
    if applied.status != "Applied"
        || applied.applied_at_ms != Some(applied.at_ms)
        || applied.interval_ms != interval
        || restored.interval_ms != interval
        || restored.applied_at_ms != applied.applied_at_ms
        || restored.expires_at_ms != applied.expires_at_ms
        || applied
            .expires_at_ms
            .checked_sub(applied.at_ms)
            .is_none_or(|d| d == 0 || d > u64::from(validity))
        || restored.status != if cancelled { "Cancelled" } else { "Restored" }
        || restored.at_ms < applied.at_ms
        || (!cancelled && restored.at_ms < restored.expires_at_ms)
    {
        bail!("periodic application/restoration contract mismatch");
    }
    // The restored demand is the latest live value, not necessarily the value
    // at admission (navigation may have changed it). Validate its shape, never
    // require revival of the original Home subscription.
    for event in [applied, restored] {
        if event.live_fields & !3 != 0
            || (event.live_fields & 1 == 0) != event.live_age0_ms.is_none()
            || (event.live_fields & 2 == 0) != event.live_age1_ms.is_none()
        {
            bail!("invalid live demand evidence");
        }
    }
    Ok(())
}

impl ObservationFixtureRuntime<'_> {
    pub(super) fn await_periodic_samples(&mut self) -> Result<()> {
        let count = self
            .cancel_after_samples
            .ok_or_else(|| anyhow!("missing cancellation sample count"))?;
        let sample = Regex::new(&format!(r"^OBSPER SAMPLE id={}(?: |$)", self.id))?;
        let terminal = Regex::new(&format!(r"^OBSPER RESULT id={}(?: |$)", self.id))?;
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            self.console.poll_once()?;
            if self.console.has_regex_since(self.command_mark, &terminal) {
                bail!("periodic fixture ended before requested cancellation point");
            }
            if self.console.count_regex_since(self.command_mark, &sample) >= count {
                return Ok(());
            }
        }
        bail!("timed out awaiting periodic samples; firmware expiry remains active")
    }
    pub(super) fn await_periodic_result(&mut self) -> Result<()> {
        let pattern = Regex::new(&format!(r"^OBSPER RESULT id={}(?: |$)", self.id))?;
        self.console.wait_for_regex_since(self.command_mark, &pattern, self.timeout)?
            .ok_or_else(|| anyhow!("timed out awaiting periodic restoration; no retry sent, firmware expiry remains active"))?;
        let lines = self.console.read_recent_lines(self.command_mark);
        self.periodic_evidence = Some(verify(
            &lines,
            self.id,
            self.provider,
            self.period_ms.unwrap(),
            self.validity_ms,
            self.cancel_after_samples.is_some(),
        )?);
        Ok(())
    }
}

pub(super) fn validate(
    period_ms: Option<u32>,
    cancel: Option<usize>,
    validity_ms: u32,
) -> Result<()> {
    match period_ms {
        Some(period)
            if (60_000..=300_000).contains(&period)
                && validity_ms > period
                && validity_ms <= 900_000 => {}
        Some(_) => bail!("period-ms must be 60000..=300000 and below validity-ms (maximum 900000)"),
        None if cancel.is_some() => bail!("cancel-after-samples requires period-ms"),
        None => return Ok(()),
    }
    if cancel.is_some_and(|n| !(2..=15).contains(&n)) {
        bail!("cancel-after-samples must be 2..=15");
    }
    Ok(())
}
