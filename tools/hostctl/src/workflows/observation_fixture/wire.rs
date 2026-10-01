//! Correlated OBSFIX results and the evidence needed for a fresh typed sample.

use std::collections::BTreeMap;

use super::FixtureProvider;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct FixtureResult {
    pub id: u64,
    pub provider: u64,
    pub status: String,
    pub sample: Option<SampleEvidence>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SampleEvidence {
    pub fields: u64,
    pub owner_generation: u64,
    pub admitted_at_ms: u64,
    pub expires_at_ms: u64,
    pub generation: u64,
    pub revision: u64,
    pub health: String,
    pub last_attempt_at_ms: Option<u64>,
    pub last_sample_at_ms: Option<u64>,
    pub last_sample_revision: Option<u64>,
    pub baseline_generation: Option<u64>,
    pub baseline_sample_revision: Option<u64>,
    pub percent: Option<u64>,
    pub temperature_centidegrees: Option<i32>,
    pub humidity_millipercent: Option<i64>,
    pub temperature_millicelsius: Option<i32>,
    pub millivolts: Option<u32>,
}

pub(crate) type Fields<'a> = BTreeMap<&'a str, &'a str>;

pub(crate) fn field<'a>(fields: &Fields<'a>, name: &str) -> Result<&'a str> {
    fields
        .get(name)
        .copied()
        .ok_or_else(|| anyhow!("OBSFIX result missing {name}"))
}

pub(crate) fn number(fields: &Fields<'_>, name: &str) -> Result<u64> {
    field(fields, name)?
        .parse()
        .with_context(|| format!("invalid OBSFIX numeric field {name}"))
}

pub(crate) fn optional_number(fields: &Fields<'_>, name: &str) -> Result<Option<u64>> {
    match field(fields, name)? {
        "none" => Ok(None),
        _ => number(fields, name).map(Some),
    }
}

fn optional_value<T: std::str::FromStr>(fields: &Fields<'_>, name: &str) -> Result<Option<T>> {
    match field(fields, name)? {
        "none" => Ok(None),
        value => value
            .parse()
            .map(Some)
            .map_err(|_| anyhow!("invalid OBSFIX numeric field {name}")),
    }
}

pub(crate) fn parse_fields(body: &str) -> Result<Fields<'_>> {
    let mut fields = Fields::new();
    for token in body.split_whitespace() {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| anyhow!("malformed OBSFIX result field"))?;
        if fields.insert(key, value).is_some() {
            bail!("duplicate OBSFIX result field {key}");
        }
    }
    Ok(fields)
}

pub(crate) fn parse_result(line: &str, expected: FixtureProvider) -> Result<FixtureResult> {
    let body = line
        .strip_prefix("OBSFIX RESULT ")
        .ok_or_else(|| anyhow!("not an OBSFIX RESULT line"))?;
    let fields = parse_fields(body)?;
    let id = number(&fields, "id")?;
    if id == 0 {
        bail!("OBSFIX result id must be nonzero");
    }
    let provider = number(&fields, "provider")?;
    let status = field(&fields, "status")?.to_owned();
    if !matches!(
        status.as_str(),
        "Sampled"
            | "Expired"
            | "Cancelled"
            | "Restarted"
            | "Busy"
            | "Invalid"
            | "StaleId"
            | "GenerationExhausted"
            | "Full"
            | "Closed"
            | "Unavailable"
    ) {
        bail!("unknown OBSFIX terminal status {status}");
    }
    // Rejections may contain only the correlated identity and status.
    let sample = if status == "Sampled" {
        Some(parse_sample(&fields, expected)?)
    } else {
        None
    };
    Ok(FixtureResult {
        id,
        provider,
        status,
        sample,
    })
}

fn parse_sample(fields: &Fields<'_>, expected: FixtureProvider) -> Result<SampleEvidence> {
    Ok(SampleEvidence {
        fields: number(fields, "fields")?,
        owner_generation: number(fields, "owner_generation")?,
        admitted_at_ms: number(fields, "admitted_at_ms")?,
        expires_at_ms: number(fields, "expires_at_ms")?,
        generation: number(fields, "generation")?,
        revision: number(fields, "revision")?,
        health: field(fields, "health")?.to_owned(),
        last_attempt_at_ms: optional_number(fields, "last_attempt_at_ms")?,
        last_sample_at_ms: optional_number(fields, "last_sample_at_ms")?,
        last_sample_revision: optional_number(fields, "last_sample_revision")?,
        baseline_generation: optional_number(fields, "baseline_generation")?,
        baseline_sample_revision: optional_number(fields, "baseline_sample_revision")?,
        percent: if matches!(expected, FixtureProvider::Battery | FixtureProvider::Adc) {
            optional_number(fields, "percent")?
        } else {
            None
        },
        temperature_centidegrees: if matches!(expected, FixtureProvider::Bme688) {
            optional_value(fields, "temperature_centidegrees")?
        } else {
            None
        },
        temperature_millicelsius: if matches!(expected, FixtureProvider::Shtc3) {
            optional_value(fields, "temperature_millicelsius")?
        } else {
            None
        },
        millivolts: if matches!(expected, FixtureProvider::Adc) {
            optional_value(fields, "millivolts")?
        } else {
            None
        },
        humidity_millipercent: match expected {
            FixtureProvider::Bme688 => {
                optional_value::<u32>(fields, "humidity_millipercent")?.map(i64::from)
            }
            FixtureProvider::Shtc3 => {
                optional_value::<i32>(fields, "humidity_millipercent")?.map(i64::from)
            }
            _ => None,
        },
    })
}

pub(crate) fn verify_sample(
    result: &FixtureResult,
    id: u64,
    validity_ms: u32,
    provider: FixtureProvider,
) -> Result<()> {
    if result.id != id || result.provider != provider.id() {
        bail!("OBSFIX result identity/provider mismatch");
    }
    if result.status != "Sampled" {
        bail!(
            "OBSFIX request {id} terminated with status={}",
            result.status
        );
    }
    let sample = result
        .sample
        .as_ref()
        .ok_or_else(|| anyhow!("OBSFIX Sampled result has no sample metadata"))?;
    sample
        .expires_at_ms
        .checked_sub(sample.admitted_at_ms)
        .filter(|duration| *duration > 0 && *duration <= u64::from(validity_ms))
        .ok_or_else(|| anyhow!("OBSFIX admission/expiry exceeds requested validity"))?;
    let sampled_at = sample
        .last_sample_at_ms
        .ok_or_else(|| anyhow!("OBSFIX Sampled result has no successful sample timestamp"))?;
    let sample_revision = sample
        .last_sample_revision
        .filter(|revision| *revision > 0 && *revision <= sample.revision)
        .ok_or_else(|| anyhow!("OBSFIX successful sample revision is missing or invalid"))?;
    let attempted_at = sample
        .last_attempt_at_ms
        .ok_or_else(|| anyhow!("OBSFIX Sampled result has no attempt timestamp"))?;
    if sample.fields != provider.fields()
        || sample.owner_generation == 0
        || sampled_at < sample.admitted_at_ms
        || sampled_at >= sample.expires_at_ms
        || attempted_at < sampled_at
        || !matches!(sample.health.as_str(), "Ok" | "Degraded")
    {
        bail!("OBSFIX successful sample metadata violates the request contract");
    }
    match provider {
        FixtureProvider::Battery | FixtureProvider::Adc
            if !sample.percent.is_some_and(|percent| percent <= 100) =>
        {
            bail!("OBSFIX battery percent missing or out of range")
        }
        FixtureProvider::Bme688
            if sample.temperature_centidegrees.is_none()
                || sample.humidity_millipercent.is_none() =>
        {
            bail!("OBSFIX BME688 combined values missing")
        }
        FixtureProvider::Shtc3
            if sample.temperature_millicelsius.is_none()
                || sample.humidity_millipercent.is_none() =>
        {
            bail!("OBSFIX SHTC3 combined values missing")
        }
        FixtureProvider::Adc if sample.millivolts.is_none() => {
            bail!("OBSFIX ADC voltage missing")
        }
        _ => {}
    }
    match (sample.baseline_generation, sample.baseline_sample_revision) {
        (Some(generation), Some(revision)) => {
            if generation != sample.generation || sample_revision <= revision {
                bail!("OBSFIX sample must advance within the admission provider generation");
            }
        }
        (None, None) => {}
        _ => bail!("OBSFIX baseline sample identity is incomplete"),
    }
    Ok(())
}
