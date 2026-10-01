//! `buzzer_preview`: render a shared sequencer score to a nominal WAV.
//!
//! Usage:
//!
//! ```text
//! buzzer_preview --out logs/ding.wav "880:90 0:40 1175:110"
//! buzzer_preview --out logs/ack.wav --rtttl "ack:d=16,o=6,b=120:c,32p,e"
//! buzzer_preview --out logs/ack.wav --rtttl-file ack.rtttl
//! ```
//!
//! `ack.rtttl` above is a placeholder file holding exactly one RTTTL line
//! (the shared `assets/sounds/notifications.rtttl` catalogue holds three,
//! so it is not valid input for this flag).
//!
//! The score is either `HZ:MS` notes separated by spaces or commas, with
//! `0:MS` for rests, or one RTTTL string (`--rtttl`) optionally transposed
//! as a whole score (`--transpose SEMITONES`). `--rtttl-file` reads that one
//! RTTTL string from a file holding exactly one RTTTL line (blank and `#`
//! comment lines skipped). The command prints the deterministic
//! action trace (the same plan the firmware runner executes, plus its
//! terminal shutdown) and writes a 16-bit mono WAV at [`SAMPLE_RATE_HZ`].
//! All acoustic content beyond nominal square-wave timing is provisional;
//! see the library docs.

use buzzer::runner::record;
use buzzer::score::{Element, RequestedAction};
use buzzer_preview::{
    nominal_freq_hz, parse_preview_cli, parse_score_arg, render_score, write_wav, ScoreSource,
    AMPLITUDE, SAMPLE_RATE_HZ,
};
use std::process::ExitCode;

fn print_help() -> ExitCode {
    println!(
        "usage: buzzer_preview --out <file.wav> \"HZ:MS[ HZ:MS...]\"\n       \
         buzzer_preview --out <file.wav> --rtttl \"name:d=..,o=..,b=..:notes\" [--transpose N]\n       \
         buzzer_preview --out <file.wav> --rtttl-file <one-line.rtttl> [--transpose N]\n       \
         0:MS is a rest; HZ frequencies map to nominal codes (out-of-range saturates);\n       \
         RTTTL requests outside the nominal range are rejected, never clamped"
    );
    ExitCode::SUCCESS
}

fn load_elements(source: &ScoreSource, transpose: i32) -> Result<Vec<Element>, String> {
    match source {
        ScoreSource::HzMs(spec) => parse_score_arg(spec),
        ScoreSource::RtttlText(text) => {
            buzzer_preview::rtttl::compile(text, transpose).map_err(|error| error.to_string())
        }
        ScoreSource::RtttlFile(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read {path}: {error}"))?;
            let lines: Vec<&str> = text
                .lines()
                .map(|line| line.strip_suffix('\r').unwrap_or(line).trim())
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect();
            if lines.len() != 1 {
                return Err(format!(
                    "--rtttl-file expects exactly one RTTTL line, found {}",
                    lines.len()
                ));
            }
            buzzer_preview::rtttl::compile(lines[0], transpose).map_err(|error| error.to_string())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return print_help();
    }
    let cli = match parse_preview_cli(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("buzzer_preview: {error}");
            return ExitCode::FAILURE;
        }
    };
    let elements = match load_elements(&cli.source, cli.transpose) {
        Ok(elements) => elements,
        Err(error) => {
            eprintln!("buzzer_preview: {error}");
            return ExitCode::FAILURE;
        }
    };
    let total_ms = buzzer::score::total_duration_ms(&elements);
    let sample_count = total_ms as usize * SAMPLE_RATE_HZ as usize / 1000;
    let mut samples = vec![0i16; sample_count];
    let mut actions = vec![
        RequestedAction {
            at_ms: 0,
            kind: buzzer::score::ActionKind::PowerOff,
        };
        elements.len() + 1
    ];
    let rendered = render_score(&elements, &mut samples, &mut actions);
    // Deterministic trace first: CI compares this, not the audio bytes.
    let trace = record(&elements, &mut actions).expect("trace buffer fits by construction");
    for action in &actions[..trace.actions] {
        let (kind, code) = match action.kind {
            buzzer::score::ActionKind::PowerOnSetCode { code } => ("power_on", code as u32),
            buzzer::score::ActionKind::SetCode { code } => ("pitch", code as u32),
            buzzer::score::ActionKind::PowerOff => ("power_off", 0),
        };
        let hz = if matches!(action.kind, buzzer::score::ActionKind::PowerOff) {
            0
        } else {
            nominal_freq_hz(code as u8).unwrap_or(0)
        };
        println!(
            "BUZZ t_ms={} op={kind} code={code} nominal_hz={hz} provisional=acoustic-filtering-unmeasured",
            action.at_ms,
        );
    }
    let wav = write_wav(&samples[..rendered.samples], SAMPLE_RATE_HZ);
    if let Err(error) = std::fs::write(&cli.out, &wav) {
        eprintln!("buzzer_preview: cannot write {}: {error}", cli.out);
        return ExitCode::FAILURE;
    }
    println!(
        "WAV samples={} actions={} total_ms={} amplitude={AMPLITUDE} path={}",
        rendered.samples, rendered.actions, rendered.total_ms, cli.out,
    );
    ExitCode::SUCCESS
}
