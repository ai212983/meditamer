use anyhow::{anyhow, Result};
use regex::Regex;

/// Exact device marker lines, keyed by the short key the YAML passes.
pub const MARKER_TABLE: &[(&str, &str)] = &[
    (
        "ambient_assets_adopted",
        r"^AMBIENT_ASSETS status=adopted(\s|$)",
    ),
    ("ambient_entered", r"^AMBIENT_SCREEN status=entered(\s|$)"),
    (
        "mountain_cache_validated",
        r"^MOUNTAIN_CACHE status=validated bytes=665839 chunks=21(\s|$)",
    ),
    (
        "mountain_released",
        r"^MOUNTAIN_CACHE status=released reason=screen_exit bytes=665839(\s|$)",
    ),
    (
        "mountain_validated",
        r"^MOUNTAIN_STREAM status=validated(\s|$)",
    ),
    (
        "mountain_rendered",
        r"^MOUNTAIN_STREAM status=rendered(\s|$)",
    ),
    (
        "mountain_adopted",
        r"^AMBIENT_MOUNTAIN status=adopted(\s|$)",
    ),
    (
        "mountain_composed",
        r"^AMBIENT_MOUNTAIN status=composed(\s|$)",
    ),
    ("clock_entered", r"^CLOCK_SCREEN status=entered(\s|$)"),
    (
        "clock_assets_ready",
        // A complete frame proves assets were available, but is later than
        // asset readiness. Keep that implication separate so timing captures
        // cannot mistake a dropped UART line for a measured load duration.
        r"^CLOCK_ASSETS status=(complete|borrowed)(\s|$)",
    ),
    (
        "clock_frame_complete",
        r"^CLOCK_FRAME status=complete(\s|$)",
    ),
    ("clock_profile", r"^CLOCK_PROFILE(\s|$)"),
    (
        "clock_assets_released",
        r"^CLOCK_ASSETS status=released(\s|$)",
    ),
];

pub fn marker_pattern(key: &str) -> Result<Regex> {
    MARKER_TABLE
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, pattern)| Regex::new(pattern))
        .ok_or_else(|| anyhow!("unknown asset marker key: {key}"))?
        .map_err(|err| anyhow!("invalid asset marker pattern for {key}: {err}"))
}
