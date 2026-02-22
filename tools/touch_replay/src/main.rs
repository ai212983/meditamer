use std::{
    env,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process,
};

#[path = "../../../src/app/touch/core.rs"]
mod touch_core;

use touch_core::{
    TouchEngine, TouchEvent, TouchEventKind, TouchSample, TouchSwipeDirection, TouchPoint,
};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(usage());
    }

    let mut trace_path: Option<PathBuf> = None;
    let mut expect_path: Option<PathBuf> = None;

    let mut idx = 1usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--expect" => {
                idx += 1;
                let Some(path) = args.get(idx) else {
                    return Err("missing path after --expect".into());
                };
                expect_path = Some(PathBuf::from(path));
            }
            "-h" | "--help" => {
                println!("{}", usage());
                return Ok(());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown argument: {value}"));
            }
            value => {
                if trace_path.is_some() {
                    return Err("multiple trace paths provided".into());
                }
                trace_path = Some(PathBuf::from(value));
            }
        }
        idx += 1;
    }

    let trace_path = trace_path.ok_or_else(usage)?;
    let samples = parse_trace(&trace_path)?;

    let mut engine = TouchEngine::new();
    let mut events: Vec<TouchEvent> = Vec::new();
    for (ms, sample) in samples {
        let output = engine.tick(ms, sample);
        for event in output.events.into_iter().flatten() {
            events.push(event);
        }
    }

    println!("event,ms,kind,x,y,start_x,start_y,duration_ms,count");
    for event in &events {
        println!(
            "event,{},{},{},{},{},{},{},{}",
            event.t_ms,
            kind_label(event.kind),
            event.x,
            event.y,
            event.start_x,
            event.start_y,
            event.duration_ms,
            event.touch_count
        );
    }

    if let Some(expect_path) = expect_path {
        let expected = parse_expected_kinds(&expect_path)?;
        let actual: Vec<&'static str> = events.iter().map(|e| kind_label(e.kind)).collect();
        if actual != expected {
            eprintln!("expected kinds: {}", expected.join(","));
            eprintln!("actual kinds:   {}", actual.join(","));
            return Err("event sequence mismatch".into());
        }
    }

    Ok(())
}

fn usage() -> String {
    "usage: touch_replay <trace.csv> [--expect expected_kinds.txt]".to_string()
}

fn parse_trace(path: &Path) -> Result<Vec<(u64, TouchSample)>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);

    let mut out = Vec::new();
    for (line_no, line_result) in reader.lines().enumerate() {
        let line_no = line_no + 1;
        let line = line_result.map_err(|e| {
            format!(
                "failed to read {}:{}: {e}",
                path.display(),
                line_no
            )
        })?;
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

        out.push((
            ms,
            TouchSample {
                touch_count: count,
                points: [TouchPoint { x: x0, y: y0 }, TouchPoint { x: x1, y: y1 }],
            },
        ));
    }

    Ok(out)
}

fn parse_expected_kinds(path: &Path) -> Result<Vec<&'static str>, String> {
    let file = File::open(path).map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    let reader = BufReader::new(file);

    let mut kinds = Vec::new();
    for (line_no, line_result) in reader.lines().enumerate() {
        let line_no = line_no + 1;
        let line = line_result.map_err(|e| {
            format!(
                "failed to read {}:{}: {e}",
                path.display(),
                line_no
            )
        })?;
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

fn kind_label(kind: TouchEventKind) -> &'static str {
    match kind {
        TouchEventKind::Down => "down",
        TouchEventKind::Move => "move",
        TouchEventKind::Up => "up",
        TouchEventKind::Tap => "tap",
        TouchEventKind::LongPress => "long_press",
        TouchEventKind::Swipe(TouchSwipeDirection::Left) => "swipe_left",
        TouchEventKind::Swipe(TouchSwipeDirection::Right) => "swipe_right",
        TouchEventKind::Swipe(TouchSwipeDirection::Up) => "swipe_up",
        TouchEventKind::Swipe(TouchSwipeDirection::Down) => "swipe_down",
        TouchEventKind::Cancel => "cancel",
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
