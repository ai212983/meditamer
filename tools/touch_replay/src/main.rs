use std::{env, path::PathBuf, process};

mod kinds;
mod parse;

#[path = "../../../src/firmware/touch/core/mod.rs"]
mod touch_core;
#[path = "../../../src/firmware/touch/normalize/mod.rs"]
mod touch_normalize;

use kinds::kind_label;
use parse::{parse_expected_kinds, parse_trace};
use touch_core::{TouchEngine, TouchEvent, TouchPoint, TouchSample};
use touch_normalize::{NormalizedTouchPoint, NormalizedTouchSample, TouchPresenceNormalizer};

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
    let last_sample_ms = samples.last().map(|s| s.ms);

    let mut normalizer = TouchPresenceNormalizer::new();
    let mut engine = TouchEngine::new();
    let mut events: Vec<TouchEvent> = Vec::new();
    for replay in &samples {
        let normalized = NormalizedTouchSample {
            touch_count: replay.touch_count,
            points: [
                NormalizedTouchPoint {
                    x: replay.points[0].x,
                    y: replay.points[0].y,
                },
                NormalizedTouchPoint {
                    x: replay.points[1].x,
                    y: replay.points[1].y,
                },
            ],
            raw: replay.raw,
        };
        let (count, primary) = normalizer.normalize(replay.ms, normalized);
        let core_sample = TouchSample {
            touch_count: count,
            points: [
                primary
                    .map(|p| TouchPoint { x: p.x, y: p.y })
                    .unwrap_or_default(),
                TouchPoint::default(),
            ],
        };

        let output = engine.tick(replay.ms, core_sample);
        for event in output.events.into_iter().flatten() {
            events.push(event);
        }
    }

    // Ensure release/debounce states can flush final Up/Tap/Swipe decisions even
    // when captured traces stop immediately after the last physical sample.
    if let Some(last_ms) = events.last().map(|e| e.t_ms).or(last_sample_ms) {
        let tail_ms = last_ms.saturating_add(200);
        let (count, primary) = normalizer.advance(tail_ms);
        let core_sample = TouchSample {
            touch_count: count,
            points: [
                primary
                    .map(|p| TouchPoint { x: p.x, y: p.y })
                    .unwrap_or_default(),
                TouchPoint::default(),
            ],
        };
        let output = engine.tick(tail_ms, core_sample);
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
