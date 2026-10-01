//! Probe-mode selection and shared run constants.
//!
//! Every timing candidate is fixed at compile time so each binary has one
//! inspectable hot loop. This module owns the mode switch, the run-cycle and
//! logging policy, and the tiny `option_env!` integer parser behind them.

use crate::probe_config;

pub(crate) const RUN_CYCLES: u32 = parse_u32(
    option_env!("MEDITAMER_PANEL_SOAK_CYCLES"),
    probe_config::PARTIAL_SAMPLES,
);
pub(crate) const REFRESH_INTERVAL_MS: u64 = parse_u32(
    option_env!("MEDITAMER_PANEL_SOAK_REFRESH_INTERVAL_MS"),
    probe_config::REFRESH_INTERVAL_MS,
) as u64;
pub(crate) const INSPECTION_CHECKPOINT_EVERY: u32 =
    parse_u32(option_env!("MEDITAMER_PANEL_SOAK_CHECKPOINT_EVERY"), 0);
pub(crate) const INSPECTION_HOLD_MS: u64 =
    parse_u32(option_env!("MEDITAMER_PANEL_SOAK_CHECKPOINT_HOLD_MS"), 0) as u64;
pub(crate) const COMPACT_LOG: bool = option_env!("MEDITAMER_PANEL_SOAK_COMPACT_LOG").is_some();
pub(crate) const PANEL_I2C_KHZ: u32 = if option_env!("MEDITAMER_PANEL_I2C_400_KHZ").is_some() {
    400
} else {
    100
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProbeMode {
    Partial,
    PartialPerformance,
    PartialProfile,
    PartialSpans,
    FullPerformanceBinary,
    FullPerformanceGray3,
    FullProfileBinary,
    FullProfileGray3,
    FullSoak,
    PartialThenFull,
    Ghosting,
}

pub(crate) const PROBE_MODE: ProbeMode =
    if option_env!("MEDITAMER_PANEL_BENCHMARK_PARTIAL_1BIT").is_some() {
        ProbeMode::PartialPerformance
    } else if option_env!("MEDITAMER_PANEL_PROFILE_PARTIAL_1BIT").is_some() {
        ProbeMode::PartialProfile
    } else if option_env!("MEDITAMER_PANEL_BENCHMARK_FULL_1BIT").is_some() {
        ProbeMode::FullPerformanceBinary
    } else if option_env!("MEDITAMER_PANEL_BENCHMARK_FULL_3BIT").is_some() {
        ProbeMode::FullPerformanceGray3
    } else if option_env!("MEDITAMER_PANEL_PROFILE_FULL_1BIT").is_some() {
        ProbeMode::FullProfileBinary
    } else if option_env!("MEDITAMER_PANEL_PROFILE_FULL_3BIT").is_some() {
        ProbeMode::FullProfileGray3
    } else if option_env!("MEDITAMER_PANEL_SOAK_PARTIAL_SPANS").is_some() {
        ProbeMode::PartialSpans
    } else if option_env!("MEDITAMER_PANEL_SOAK_GHOSTING").is_some() {
        ProbeMode::Ghosting
    } else if option_env!("MEDITAMER_PANEL_SOAK_FULL_ONLY").is_some() {
        ProbeMode::FullSoak
    } else if option_env!("MEDITAMER_PANEL_SOAK_PARTIAL_THEN_FULL").is_some() {
        ProbeMode::PartialThenFull
    } else {
        ProbeMode::Partial
    };

pub(crate) const fn probe_purpose(mode: ProbeMode) -> &'static str {
    match mode {
        ProbeMode::PartialPerformance
        | ProbeMode::FullPerformanceBinary
        | ProbeMode::FullPerformanceGray3 => "performance",
        ProbeMode::PartialProfile | ProbeMode::FullProfileBinary | ProbeMode::FullProfileGray3 => {
            "phase_profile"
        }
        ProbeMode::FullSoak | ProbeMode::PartialThenFull | ProbeMode::Ghosting => "corruption_soak",
        ProbeMode::Partial | ProbeMode::PartialSpans => "timing_or_soak",
    }
}

pub(crate) const fn benchmark_name(mode: ProbeMode) -> &'static str {
    match mode {
        ProbeMode::PartialPerformance | ProbeMode::PartialProfile => "partial_1bit",
        ProbeMode::FullPerformanceBinary => "full_1bit",
        ProbeMode::FullPerformanceGray3 => "full_3bit",
        ProbeMode::FullProfileBinary => "full_1bit",
        ProbeMode::FullProfileGray3 => "full_3bit",
        _ => "none",
    }
}

pub(crate) const fn parse_u32(value: Option<&str>, fallback: u32) -> u32 {
    let Some(value) = value else {
        return fallback;
    };
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return fallback;
    }
    let mut parsed = 0u32;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte < b'0' || byte > b'9' {
            return fallback;
        }
        let Some(multiplied) = parsed.checked_mul(10) else {
            return fallback;
        };
        let Some(next) = multiplied.checked_add((byte - b'0') as u32) else {
            return fallback;
        };
        parsed = next;
        index += 1;
    }
    parsed
}

pub(crate) fn should_emit_cycle(cycle: u32) -> bool {
    !COMPACT_LOG || cycle == 1 || cycle == RUN_CYCLES || cycle.is_multiple_of(50)
}
