use anyhow::{anyhow, bail, Result};
use regex::Regex;
use std::time::Duration;

use super::BlePhase1sRuntime;
use crate::serial_console::SerialConsole;

const INTERNAL_RESERVE_BYTES: u32 = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AllocatorMinimum {
    current_bytes: u32,
    pub(super) free_bytes: u32,
    pub(super) charge_bytes: u32,
    pub(super) internal_required: bool,
    charge_overflow: bool,
    post_free_bytes: u32,
    pub(super) correlation_stable: bool,
    pub(super) wifi_rx_matched: bool,
    pub(super) released: bool,
    probe_performed: bool,
    probe_block: u32,
    probe_reserve: u32,
    probe_before: u32,
    probe_after: u32,
    probe_stable: bool,
}

pub(super) fn allocator_status_regex() -> Result<Regex> {
    Ok(Regex::new(
        r"PSRAM [^\r\n]*internal_free_bytes=([0-9]+) [^\r\n]*min_internal_free_bytes=([0-9]+) min_internal_alloc_charge_bytes=([0-9]+) min_internal_alloc_internal_required=(true|false) min_internal_alloc_charge_overflow=(true|false) min_internal_alloc_post_free_bytes=([0-9]+) min_internal_alloc_correlation_stable=(true|false) min_internal_alloc_wifi_rx_matched=(true|false) min_internal_alloc_released=(true|false) [^\r\n]*internal_probe_performed=(true|false) internal_probe_block_bytes=([0-9]+) internal_probe_reserve_bytes=([0-9]+) internal_probe_free_before_bytes=([0-9]+) internal_probe_free_after_bytes=([0-9]+) internal_probe_stable=(true|false)$",
    )?)
}

pub(super) fn parse_allocator_minimum(line: &str, regex: &Regex) -> Result<AllocatorMinimum> {
    let captures = regex
        .captures(line)
        .ok_or_else(|| anyhow!("invalid correlated allocator status: {line}"))?;
    let current = captures
        .get(1)
        .ok_or_else(|| anyhow!("allocator status omitted current internal free bytes"))?
        .as_str()
        .parse::<u32>()?;
    let minimum = captures
        .get(2)
        .ok_or_else(|| anyhow!("allocator status omitted internal-free minimum"))?
        .as_str()
        .parse::<u32>()?;
    let charge = captures
        .get(3)
        .ok_or_else(|| anyhow!("allocator status omitted low-water allocation charge"))?
        .as_str()
        .parse::<u32>()?;
    let internal_required = captures
        .get(4)
        .ok_or_else(|| anyhow!("allocator status omitted low-water internal requirement"))?
        .as_str()
        .parse::<bool>()?;
    let charge_overflow = captures
        .get(5)
        .ok_or_else(|| anyhow!("allocator status omitted low-water charge overflow"))?
        .as_str()
        .parse::<bool>()?;
    let post_free = captures
        .get(6)
        .ok_or_else(|| anyhow!("allocator status omitted low-water post-allocation free bytes"))?
        .as_str()
        .parse::<u32>()?;
    let (correlation_stable, wifi_rx_matched, released) = parse_correlation(&captures)?;
    let probe_performed = captures
        .get(10)
        .ok_or_else(|| anyhow!("allocator status omitted probe disposition"))?
        .as_str()
        .parse::<bool>()?;
    let probe_block = captures
        .get(11)
        .ok_or_else(|| anyhow!("allocator status omitted probe block"))?
        .as_str()
        .parse::<u32>()?;
    let probe_reserve = captures
        .get(12)
        .ok_or_else(|| anyhow!("allocator status omitted probe reserve"))?
        .as_str()
        .parse::<u32>()?;
    let probe_before = captures
        .get(13)
        .ok_or_else(|| anyhow!("allocator status omitted pre-snapshot free bytes"))?
        .as_str()
        .parse::<u32>()?;
    let probe_after = captures
        .get(14)
        .ok_or_else(|| anyhow!("allocator status omitted post-snapshot free bytes"))?
        .as_str()
        .parse::<u32>()?;
    let probe_stable = captures
        .get(15)
        .ok_or_else(|| anyhow!("allocator status omitted probe stability"))?
        .as_str()
        .parse::<bool>()?;
    Ok(AllocatorMinimum {
        current_bytes: current,
        free_bytes: minimum,
        charge_bytes: charge,
        internal_required,
        charge_overflow,
        post_free_bytes: post_free,
        correlation_stable,
        wifi_rx_matched,
        released,
        probe_performed,
        probe_block,
        probe_reserve,
        probe_before,
        probe_after,
        probe_stable,
    })
}

pub(super) fn validate_allocator_minimum(minimum: &AllocatorMinimum) -> Result<()> {
    let coherent = [
        minimum.charge_bytes != 0,
        !minimum.charge_overflow,
        minimum.post_free_bytes == minimum.free_bytes,
        minimum.correlation_stable,
        minimum.released,
        !minimum.probe_performed,
        minimum.probe_block == 0,
        minimum.probe_reserve == INTERNAL_RESERVE_BYTES,
        minimum.probe_before == minimum.probe_after,
        minimum.probe_before == minimum.current_bytes,
        minimum.free_bytes <= minimum.current_bytes,
        minimum.probe_stable,
    ]
    .into_iter()
    .all(|condition| condition);
    if !coherent {
        bail!(
            "allocator status did not use coherent low-water provenance and an allocation-free serving snapshot: current={} minimum={} charge={} internal_required={} charge_overflow={} post_free={} correlation_stable={} wifi_rx_matched={} released={} performed={} block={} reserve={} before={} after={} stable={}",
            minimum.current_bytes,
            minimum.free_bytes,
            minimum.charge_bytes,
            minimum.internal_required,
            minimum.charge_overflow,
            minimum.post_free_bytes,
            minimum.correlation_stable,
            minimum.wifi_rx_matched,
            minimum.released,
            minimum.probe_performed,
            minimum.probe_block,
            minimum.probe_reserve,
            minimum.probe_before,
            minimum.probe_after,
            minimum.probe_stable
        );
    }
    Ok(())
}

fn parse_correlation(captures: &regex::Captures<'_>) -> Result<(bool, bool, bool)> {
    let required_bool = |index, field| -> Result<bool> {
        captures
            .get(index)
            .ok_or_else(|| anyhow!("allocator status omitted {field}"))?
            .as_str()
            .parse::<bool>()
            .map_err(anyhow::Error::from)
    };
    Ok((
        required_bool(7, "correlation stability")?,
        required_bool(8, "Wi-Fi RX match")?,
        required_bool(9, "allocation release state")?,
    ))
}

pub(super) fn query_allocator_minimum(console: &mut SerialConsole) -> Result<AllocatorMinimum> {
    let status_re = allocator_status_regex()?;
    let status_line = console
        .command_wait_regex("ALLOCSTATUS", &status_re, Duration::from_secs(5))?
        .ok_or_else(|| anyhow!("missing correlated allocator status"))?;
    parse_allocator_minimum(&status_line, &status_re)
}

pub(super) fn collect_stack_metrics(runtime: &mut BlePhase1sRuntime<'_>) -> Result<()> {
    let (main, touch, uart_log_drops) = runtime.query_stack_status()?;
    runtime.cpu0_stack_headroom_min = Some(main);
    runtime.touch_stack_headroom_min = Some(touch);
    runtime.uart_log_drops_final = Some(uart_log_drops);
    runtime.reject_reset_or_panic_since_evidence_mark("stack-floor collection")?;
    let minimum = query_allocator_minimum(&mut runtime.console)?;
    runtime.serving_internal_free_min = Some(minimum.free_bytes);
    runtime.serving_internal_min_alloc_charge = Some(minimum.charge_bytes);
    runtime.serving_internal_min_alloc_internal_required = Some(minimum.internal_required);
    runtime.serving_internal_min_alloc_wifi_rx_matched = Some(minimum.wifi_rx_matched);
    runtime.serving_internal_min_alloc_correlation_stable = Some(minimum.correlation_stable);
    runtime.serving_internal_min_alloc_released = Some(minimum.released);
    validate_allocator_minimum(&minimum)?;
    runtime.reject_reset_or_panic_since_evidence_mark("stack-floor collection")
}
