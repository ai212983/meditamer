use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use crate::touch_core::TouchPoint;

#[derive(Clone, Copy)]
pub(crate) struct ReplaySample {
    pub(crate) ms: u64,
    pub(crate) touch_count: u8,
    pub(crate) points: [TouchPoint; 2],
    pub(crate) raw: [u8; 8],
}

pub(crate) fn parse_trace(path: &Path) -> Result<Vec<ReplaySample>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);

    let mut out: Vec<ReplaySample> = Vec::new();
    for (line_no, line_result) in reader.lines().enumerate() {
        let line_no = line_no + 1;
        let line = line_result
            .map_err(|e| format!("failed to read {}:{}: {e}", path.display(), line_no))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "touch_trace,ms,count,x0,y0,x1,y1,raw0,raw1,raw2,raw3,raw4,raw5,raw6,raw7" {
            continue;
        }

        let parts: Vec<&str> = trimmed.split(',').collect();
        if parts.len() < 7 {
            return Err(format!(
                "{}:{} invalid trace line, expected at least 7 columns",
                path.display(),
                line_no
            ));
        }
        if parts[0].trim() != "touch_trace" {
            continue;
        }

        let ms = parse_u64(parts[1], path, line_no, "ms")?;
        let count = parse_u8(parts[2], path, line_no, "count")?;
        let x0 = parse_u16(parts[3], path, line_no, "x0")?;
        let y0 = parse_u16(parts[4], path, line_no, "y0")?;
        let x1 = parse_u16(parts[5], path, line_no, "x1")?;
        let y1 = parse_u16(parts[6], path, line_no, "y1")?;

        let mut raw = [0u8; 8];
        if parts.len() >= 15 {
            for (idx, raw_slot) in raw.iter_mut().enumerate() {
                *raw_slot = parse_u8(parts[7 + idx], path, line_no, "raw")?;
            }
        }

        out.push(ReplaySample {
            ms,
            touch_count: count,
            points: [TouchPoint { x: x0, y: y0 }, TouchPoint { x: x1, y: y1 }],
            raw,
        });
    }

    Ok(out)
}

pub(crate) fn parse_expected_kinds(path: &Path) -> Result<Vec<&'static str>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);

    let mut kinds = Vec::new();
    for (line_no, line_result) in reader.lines().enumerate() {
        let line_no = line_no + 1;
        let line = line_result
            .map_err(|e| format!("failed to read {}:{}: {e}", path.display(), line_no))?;
        let token = line.trim();
        if token.is_empty() || token.starts_with('#') {
            continue;
        }

        let normalized = normalize_kind(token).ok_or_else(|| {
            format!(
                "{}:{} invalid expected event kind: {}",
                path.display(),
                line_no,
                token
            )
        })?;
        kinds.push(normalized);
    }

    Ok(kinds)
}

fn normalize_kind(kind: &str) -> Option<&'static str> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "down" => Some("down"),
        "move" => Some("move"),
        "up" => Some("up"),
        "tap" => Some("tap"),
        "long_press" => Some("long_press"),
        "swipe_left" => Some("swipe_left"),
        "swipe_right" => Some("swipe_right"),
        "swipe_up" => Some("swipe_up"),
        "swipe_down" => Some("swipe_down"),
        "cancel" => Some("cancel"),
        _ => None,
    }
}

fn parse_u64(raw: &str, path: &Path, line_no: usize, field: &str) -> Result<u64, String> {
    raw.trim().parse::<u64>().map_err(|e| {
        format!(
            "{}:{} invalid {} '{}': {}",
            path.display(),
            line_no,
            field,
            raw.trim(),
            e
        )
    })
}

fn parse_u16(raw: &str, path: &Path, line_no: usize, field: &str) -> Result<u16, String> {
    raw.trim().parse::<u16>().map_err(|e| {
        format!(
            "{}:{} invalid {} '{}': {}",
            path.display(),
            line_no,
            field,
            raw.trim(),
            e
        )
    })
}

fn parse_u8(raw: &str, path: &Path, line_no: usize, field: &str) -> Result<u8, String> {
    raw.trim().parse::<u8>().map_err(|e| {
        format!(
            "{}:{} invalid {} '{}': {}",
            path.display(),
            line_no,
            field,
            raw.trim(),
            e
        )
    })
}
