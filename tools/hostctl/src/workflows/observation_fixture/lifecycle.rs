//! Panel interruption is a separate gate from uninterrupted periodic acquisition.
//! YAML owns preflight, admission, repaint and passive recovery observation.
use super::{
    periodic, wire, FixtureProvider, ObservationFixtureOptions, ObservationFixtureRuntime,
};
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::Serialize;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub(super) struct Live {
    fields: u64,
    age0_ms: Option<u64>,
    age1_ms: Option<u64>,
    generation: u64,
    revision: u64,
    sampled_at_ms: u64,
}
#[derive(Debug, Serialize)]
pub(super) struct Panel {
    begin_ms: u64,
    end_ms: u64,
    suspend_ids: [u64; 2],
    resume_ids: [u64; 2],
    live: Live,
}
#[derive(Debug, Serialize)]
pub(super) struct Fresh {
    generation: u64,
    revision: u64,
    sampled_at_ms: u64,
    line: String,
}
#[derive(Debug, Serialize)]
pub(super) struct Evidence {
    applied: periodic::Event,
    closed: periodic::Event,
    panel: Panel,
    fresh: Option<Fresh>,
}

fn records<'a>(
    lines: &'a [String],
    prefix: &str,
    id: u64,
) -> Result<Vec<(usize, wire::Fields<'a>)>> {
    let pattern = Regex::new(&format!(r"^{} id={id}(?: |$)", regex::escape(prefix)))?;
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| pattern.is_match(line))
        .map(|(index, line)| Ok((index, wire::parse_fields(&line[prefix.len() + 1..])?)))
        .collect()
}
fn one<'a>(lines: &'a [String], prefix: &str, id: u64) -> Result<(usize, wire::Fields<'a>)> {
    let mut entries = records(lines, prefix, id)?;
    if entries.len() != 1 {
        bail!("expected one {prefix} for id={id}, got {}", entries.len());
    }
    Ok(entries.remove(0))
}
fn control(fields: &wire::Fields<'_>, phase: &str, ack: &str) -> Result<[u64; 2]> {
    if wire::field(fields, "phase")? != phase
        || wire::field(fields, "all_ok")? != "true"
        || wire::field(fields, "environment_ack")? != ack
        || wire::field(fields, "battery_ack")? != ack
    {
        bail!("panel {phase} was not acknowledged by all clients");
    }
    let ids = [
        wire::number(fields, "environment_id")?,
        wire::number(fields, "battery_id")?,
    ];
    if ids.iter().any(|id| *id == 0 || *id > u64::from(u32::MAX)) {
        bail!("invalid control identity");
    }
    Ok(ids)
}
fn live(fields: &wire::Fields<'_>, provider: FixtureProvider) -> Result<Live> {
    if wire::number(fields, "fields")? != provider.fields()
        || wire::field(fields, "periodic_pending")? != "false"
    {
        bail!("live demand missing or periodic fixture still pending");
    }
    let live = Live {
        fields: provider.fields(),
        age0_ms: wire::optional_number(fields, "age0_ms")?,
        age1_ms: wire::optional_number(fields, "age1_ms")?,
        generation: wire::number(fields, "generation")?,
        revision: wire::number(fields, "revision")?,
        sampled_at_ms: wire::number(fields, "sampled_at_ms")?,
    };
    if live.age0_ms.is_none_or(|age| age == 0)
        || (provider.fields() == 3 && live.age1_ms.is_none_or(|age| age == 0))
        || (provider.fields() == 1 && live.age1_ms.is_some())
        || live.revision == 0
    {
        bail!("invalid live demand/sample metadata");
    }
    Ok(live)
}
fn panel(lines: &[String], id: u64, provider: FixtureProvider) -> Result<Panel> {
    let (end_index, end) = one(lines, "PANEL_FIXTURE END", id)?;
    if wire::field(&end, "status")? != "Completed" {
        bail!(
            "panel operation did not complete: {}",
            wire::field(&end, "status")?
        );
    }
    let (begin_index, begin) = one(lines, "PANEL_FIXTURE BEGIN", id)?;
    let controls = records(lines, "PANEL_FIXTURE CONTROL", id)?;
    if controls.len() != 2 {
        bail!("missing/duplicate panel control evidence");
    }
    let suspend_ids = control(&controls[0].1, "suspend", "Some(Quiesced)")?;
    let resume_ids = control(&controls[1].1, "resume", "Some(Running)")?;
    if !(begin_index < controls[0].0 && controls[0].0 < controls[1].0 && controls[1].0 < end_index)
        || suspend_ids
            .iter()
            .zip(resume_ids)
            .any(|(before, after)| *before >= after)
    {
        bail!("panel control ordering/identity mismatch");
    }
    let mut states = records(lines, "PANEL_FIXTURE LIVE", id)?;
    states.retain(|(_, fields)| wire::number(fields, "provider").ok() == Some(provider.id()));
    if states.len() != 1 || !(controls[1].0 < states[0].0 && states[0].0 < end_index) {
        bail!("missing/duplicate/out-of-order live state");
    }
    let live = live(&states[0].1, provider)?;
    let begin_ms = wire::number(&begin, "at_ms")?;
    let end_ms = wire::number(&end, "at_ms")?;
    if end_ms < begin_ms || live.sampled_at_ms > end_ms {
        bail!("invalid panel timestamps");
    }
    Ok(Panel {
        begin_ms,
        end_ms,
        suspend_ids,
        resume_ids,
        live,
    })
}

pub(super) fn verify(
    lines: &[String],
    id: u64,
    provider: FixtureProvider,
    interval: u32,
    validity: u32,
) -> Result<Evidence> {
    let panel = panel(lines, id, provider)?;
    let (applied_index, applied) = one(lines, "OBSPER APPLIED", id)?;
    let (closed_index, closed) = one(lines, "OBSPER RESULT", id)?;
    if wire::number(&applied, "provider")? != provider.id()
        || wire::number(&closed, "provider")? != provider.id()
    {
        bail!("fixture provider mismatch");
    }
    let applied = periodic::event(&applied)?;
    let closed = periodic::event(&closed)?;
    let begin_index = one(lines, "PANEL_FIXTURE BEGIN", id)?.0;
    let suspend_index = records(lines, "PANEL_FIXTURE CONTROL", id)?[0].0;
    if !(applied_index < begin_index && begin_index < closed_index && closed_index < suspend_index)
        || applied.status != "Applied"
        || closed.status != "Closed"
        || applied.applied_at_ms != Some(applied.at_ms)
        || closed.applied_at_ms != applied.applied_at_ms
        || closed.expires_at_ms != applied.expires_at_ms
        || applied.interval_ms != interval
        || closed.interval_ms != interval
        || applied
            .expires_at_ms
            .checked_sub(applied.at_ms)
            .is_none_or(|d| d == 0 || d > u64::from(validity))
        || applied.at_ms > panel.begin_ms
        || closed.at_ms < panel.begin_ms
        || closed.at_ms > panel.end_ms
        || closed.at_ms >= closed.expires_at_ms
    {
        bail!("fixture was not closed by this panel cycle within eligibility");
    }
    if closed.live_fields != panel.live.fields
        || closed.live_age0_ms != panel.live.age0_ms
        || closed.live_age1_ms != panel.live.age1_ms
    {
        bail!("live demand was not preserved after panel recovery");
    }
    Ok(Evidence {
        applied,
        closed,
        panel,
        fresh: None,
    })
}

fn some_number(fields: &wire::Fields<'_>, key: &str) -> Result<u64> {
    wire::field(fields, key)?
        .strip_prefix("Some(")
        .and_then(|v| v.strip_suffix(')'))
        .ok_or_else(|| anyhow!("missing successful sample {key}"))?
        .parse()
        .map_err(Into::into)
}
fn fresh(line: &str, evidence: &Evidence, provider: FixtureProvider) -> Result<Option<Fresh>> {
    let prefix = match provider {
        FixtureProvider::Battery => "BATTERY_DELIVER ",
        FixtureProvider::Bme688 => "BME688_DELIVER ",
        _ => bail!("Inkplate provider required"),
    };
    let Some(body) = line.strip_prefix(prefix) else {
        return Ok(None);
    };
    let fields = wire::parse_fields(body)?;
    let generation = wire::number(&fields, "generation")?;
    if generation != evidence.panel.live.generation {
        bail!("provider restarted during recovery qualification");
    }
    let sampled_at_ms = some_number(&fields, "last_sample_at_ms")?;
    let revision = some_number(&fields, "last_sample_revision")?;
    if sampled_at_ms <= evidence.panel.end_ms || revision <= evidence.panel.live.revision {
        return Ok(None);
    }
    if wire::field(&fields, "health")? != "Ok"
        || wire::number(&fields, "revision")? < revision
        || some_number(&fields, "last_attempt_at_ms")? < sampled_at_ms
    {
        bail!("fresh delivery metadata invalid");
    }
    match provider {
        FixtureProvider::Battery => {
            if wire::number(&fields, "percent")? > 100 {
                bail!("invalid battery value");
            }
        }
        FixtureProvider::Bme688 => {
            let _: i32 = wire::field(&fields, "temperature_centidegrees")?.parse()?;
            let _: u32 = wire::field(&fields, "humidity_millipercent")?.parse()?;
        }
        _ => unreachable!(),
    }
    Ok(Some(Fresh {
        generation,
        revision,
        sampled_at_ms,
        line: line.to_owned(),
    }))
}

pub(super) fn validate(opts: &ObservationFixtureOptions, id: u64) -> Result<()> {
    if opts.panel_cycle
        && (id < 2
            || !matches!(
                opts.provider,
                FixtureProvider::Battery | FixtureProvider::Bme688
            )
            || opts.period_ms.is_none()
            || opts.cancel_after_samples.is_some())
    {
        bail!("panel-cycle requires Inkplate battery/bme688, period-ms, request-id >= 2 and no cancel-after-samples");
    }
    Ok(())
}
impl ObservationFixtureRuntime<'_> {
    fn wait_panel(&mut self, id: u64, mark: usize) -> Result<Vec<String>> {
        let pattern = Regex::new(&format!(r"^PANEL_FIXTURE END id={id}(?: |$)"))?;
        self.console
            .wait_for_regex_since(mark, &pattern, self.timeout)?
            .ok_or_else(|| {
                anyhow!("timed out awaiting correlated panel completion; no retry sent")
            })?;
        Ok(self.console.read_recent_lines(mark))
    }
    pub(super) fn invoke_lifecycle(&mut self, action: &str) -> Result<()> {
        match action {
            "lifecycle_preflight" => {
                self.preflight_mark = self.console.mark();
                self.console
                    .send_line(&format!("REPAINT {}", self.id - 1))?;
            }
            "lifecycle_await_preflight" => {
                let lines = self.wait_panel(self.id - 1, self.preflight_mark)?;
                panel(&lines, self.id - 1, self.provider)?;
            }
            "lifecycle_await_panel" => {
                let lines = self.wait_panel(self.id, self.command_mark)?;
                self.lifecycle_evidence = Some(verify(
                    &lines,
                    self.id,
                    self.provider,
                    self.period_ms.unwrap(),
                    self.validity_ms,
                )?);
            }
            "lifecycle_await_fresh" => self.await_live_recovery()?,
            _ => bail!("unsupported lifecycle action {action}"),
        }
        Ok(())
    }
    fn await_live_recovery(&mut self) -> Result<()> {
        let deadline = Instant::now() + self.timeout;
        let mut next = self.command_mark;
        let evidence = self
            .lifecycle_evidence
            .as_mut()
            .ok_or_else(|| anyhow!("missing panel evidence"))?;
        while Instant::now() < deadline {
            self.console.poll_once()?;
            for line in self.console.read_recent_lines(next) {
                if line.contains("BOOT_RESET")
                    || line.contains("panic")
                    || line.contains("watchdog")
                {
                    bail!("reset/panic during lifecycle capture");
                }
                if let Some(sample) = fresh(&line, evidence, self.provider)? {
                    evidence.fresh = Some(sample);
                    return Ok(());
                }
            }
            next = self.console.mark();
        }
        bail!("timed out awaiting a fresh live delivery after panel recovery; no observe-now request or retry sent")
    }
}
