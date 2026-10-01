//! Nominal square-wave preview of shared buzzer scores.
//!
//! Renders the same [`buzzer`] plan the firmware runner executes into
//! 16-bit mono PCM: note/rest boundaries, the power-on default-code blip
//! over the startup interval, and instant power-off stops, all at the
//! nominal code frequencies. One oscillator keeps one continuous phase
//! across pitch switches; pitch changes are never rendered as independent
//! sustained partials.
//!
//! Everything acoustic beyond timing is provisional: transducer resonance,
//! enclosure filtering, oscillator transition spectra, and the power-off
//! tail are unmeasured on this board, so the preview renders them as
//! timing-preserving silence or instantaneous switches and labels them as
//! such. The preview assists iteration; device recordings decide sound
//! selection.

use buzzer::model::{freq_hz, nearest_code, OscillatorParams, CODE_MAX};
use buzzer::runner::record;
use buzzer::score::{ActionKind, Element, Note, RequestedAction};

/// Nominal Inkplate 4 TEMPERA timing network, mirroring
/// `boards/inkplate-tempera/src/buzzer.rs`: R59 = 2200 Ohm, assumed Rw =
/// 100 Ohm, RAB = 10 kOhm, C62 = 100 nF. Approximations, not calibration.
pub const INKPLATE_OSCILLATOR: OscillatorParams = OscillatorParams {
    r_base_ohm: 2300,
    r_pot_ohm: 10_000,
    code_max: CODE_MAX as u32,
};

/// Rheostat power-on default code: the timer sounds this until the first
/// pitch write lands.
pub const DEFAULT_CODE: u8 = 0x3F;

/// Rail settle before the programmed code takes over, in milliseconds.
/// Mirrors the firmware startup wait; bench evidence may change both.
pub const STARTUP_MS: u64 = 1;

/// Nominal preview amplitude: quarter scale leaves headroom and avoids any
/// implication that the preview models acoustic loudness.
pub const AMPLITUDE: i16 = 8192;

/// Preview sample rate in Hz.
pub const SAMPLE_RATE_HZ: u32 = 22_050;

/// Renders `elements` to mono PCM samples at [`SAMPLE_RATE_HZ`].
///
/// The trace driving the render is [`record`]'s terminal-inclusive plan, so
/// a trace mismatch between this preview and the firmware runner is a
/// behavioral regression in one of them. `out` too small for the plan
/// truncates the score rather than failing: previews are approximate by
/// definition, and the returned sample count always matches the actions
/// actually rendered.
pub fn render_score(
    elements: &[Element],
    out_samples: &mut [i16],
    out_actions: &mut [RequestedAction],
) -> Rendered {
    let trace = record(elements, out_actions);
    let actions: &[RequestedAction] = match trace {
        Ok(trace) => &out_actions[..trace.actions],
        Err(_) => &[],
    };
    let mut render = Rendered {
        samples: 0,
        actions: actions.len(),
        total_ms: actions.last().map_or(0, |action| action.at_ms),
    };
    if actions.is_empty() {
        return render;
    }
    let mut state = RailState {
        powered: false,
        freq_hz: 0,
        staged: None,
        phase_num: 0,
        next_action: 0,
    };
    let mut sample_index = 0usize;
    while sample_index < out_samples.len() {
        let at_ms = sample_index as u64 * 1000 / SAMPLE_RATE_HZ as u64;
        if at_ms >= render.total_ms {
            break;
        }
        while state.next_action < actions.len() && actions[state.next_action].at_ms <= at_ms {
            state.apply(actions[state.next_action]);
            state.next_action += 1;
        }
        out_samples[sample_index] = state.sample(at_ms);
        sample_index += 1;
    }
    render.samples = sample_index;
    render
}

/// What [`render_score`] produced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rendered {
    /// PCM samples written.
    pub samples: usize,
    /// Planned actions consumed, including the terminal shutdown.
    pub actions: usize,
    /// Score duration in milliseconds.
    pub total_ms: u64,
}

/// One oscillator's render state: rail power, sounding frequency, an
/// optional staged pitch write, and one continuous phase.
struct RailState {
    powered: bool,
    freq_hz: u32,
    staged: Option<(u8, u64)>,
    phase_num: u64,
    next_action: usize,
}

impl RailState {
    fn apply(&mut self, action: RequestedAction) {
        match action.kind {
            ActionKind::PowerOnSetCode { code } => {
                // Power-up runs the default code until the pitch write lands
                // after the startup wait; the switch itself is instant here
                // (provisional: the real transition spectrum is unmeasured).
                self.powered = true;
                self.freq_hz = nominal_freq_hz(DEFAULT_CODE).unwrap_or(0);
                self.staged = Some((code, action.at_ms + STARTUP_MS));
            }
            ActionKind::SetCode { code } => {
                self.freq_hz = nominal_freq_hz(code.min(CODE_MAX)).unwrap_or(0);
                self.staged = None;
            }
            ActionKind::PowerOff => {
                // Instant stop (provisional: the real power-off tail and any
                // transducer ring-down are unmeasured).
                self.powered = false;
                self.staged = None;
            }
        }
    }

    fn sample(&mut self, at_ms: u64) -> i16 {
        if !self.powered {
            return 0;
        }
        if let Some((code, effective_ms)) = self.staged {
            if at_ms >= effective_ms {
                self.freq_hz = nominal_freq_hz(code.min(CODE_MAX)).unwrap_or(0);
                self.staged = None;
            }
        }
        if self.freq_hz == 0 {
            return 0;
        }
        // Integer phase in Hz-samples, continuous across pitch switches; no
        // independent partials are modelled. The sign alternates on every
        // half cycle: doubling before the division is what makes one
        // quotient step one half period rather than one full period.
        // `phase_num * 2` overflows only after millennia of continuous
        // rendering; previews are seconds long.
        self.phase_num += self.freq_hz as u64;
        if (self.phase_num * 2 / SAMPLE_RATE_HZ as u64).is_multiple_of(2) {
            AMPLITUDE
        } else {
            -AMPLITUDE
        }
    }
}

/// Parses a compact score spec: `HZ:MS` notes separated by spaces or commas,
/// with `0:MS` for rests. Frequencies map to the nearest nominal code with
/// the shared integer model (out-of-range saturates, as on device); rests
/// need no code.
pub fn parse_score_arg(spec: &str) -> Result<Vec<Element>, String> {
    let mut elements = Vec::new();
    for token in spec.split([',', ' ']) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (hz_text, ms_text) = token
            .split_once(':')
            .ok_or_else(|| format!("expected HZ:MS, got {token:?}"))?;
        let hz: i32 = hz_text
            .parse()
            .map_err(|_| format!("bad frequency in {token:?}"))?;
        let ms: u32 = ms_text
            .parse()
            .map_err(|_| format!("bad duration in {token:?}"))?;
        if ms == 0 {
            return Err(format!("zero duration in {token:?}"));
        }
        if hz == 0 {
            elements.push(Element::Rest { duration_ms: ms });
        } else if hz < 0 {
            return Err(format!("negative frequency in {token:?}"));
        } else {
            elements.push(Element::Note(Note {
                code: nearest_code(&INKPLATE_OSCILLATOR, hz as u32),
                duration_ms: ms,
            }));
        }
    }
    if elements.is_empty() {
        return Err("empty score".to_string());
    }
    Ok(elements)
}

/// Encodes mono PCM16 samples as a WAV file.
pub fn write_wav(samples: &[i16], sample_rate_hz: u32) -> Vec<u8> {
    let data_bytes = samples.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate_hz.to_le_bytes());
    wav.extend_from_slice(&(sample_rate_hz * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Nominal frequency in Hz for a raw code, or `None` above [`CODE_MAX`].
pub fn nominal_freq_hz(code: u8) -> Option<u32> {
    if code > CODE_MAX {
        return None;
    }
    Some(freq_hz(&INKPLATE_OSCILLATOR, code))
}

/// Where a preview score comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoreSource {
    /// Legacy `HZ:MS` tokens (positional argument).
    HzMs(String),
    /// RTTTL text from `--rtttl`.
    RtttlText(String),
    /// RTTTL text read from the file named by `--rtttl-file`.
    RtttlFile(String),
}

/// Parsed `buzzer_preview` command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewCli {
    /// WAV output path from `--out`.
    pub out: String,
    /// Score source (exactly one of positional spec, `--rtttl`, `--rtttl-file`).
    pub source: ScoreSource,
    /// Whole-score semitone shift from `--transpose` (RTTTL only).
    pub transpose: i32,
}

/// Parses `buzzer_preview` arguments without touching the filesystem, so
/// invalid invocations fail before any output exists.
pub fn parse_preview_cli(args: &[String]) -> Result<PreviewCli, String> {
    let mut out: Option<String> = None;
    let mut spec: Option<String> = None;
    let mut rtttl: Option<String> = None;
    let mut rtttl_file: Option<String> = None;
    let mut transpose: Option<i32> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --out".to_string());
                }
                out = Some(args[index].clone());
            }
            "--rtttl" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --rtttl".to_string());
                }
                rtttl = Some(args[index].clone());
            }
            "--rtttl-file" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --rtttl-file".to_string());
                }
                rtttl_file = Some(args[index].clone());
            }
            "--transpose" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --transpose".to_string());
                }
                match args[index].parse::<i32>() {
                    Ok(shift) => transpose = Some(shift),
                    Err(_) => {
                        return Err(format!("bad --transpose {:?}", args[index]));
                    }
                }
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown flag {other:?}"));
            }
            other => {
                if spec.is_some() {
                    return Err("too many positional score arguments".to_string());
                }
                spec = Some(other.to_string());
            }
        }
        index += 1;
    }
    let Some(out) = out else {
        return Err("missing --out <file.wav>".to_string());
    };
    let sources =
        u8::from(spec.is_some()) + u8::from(rtttl.is_some()) + u8::from(rtttl_file.is_some());
    if sources == 0 {
        return Err("missing score (positional HZ:MS, --rtttl, or --rtttl-file)".to_string());
    }
    if sources > 1 {
        return Err("pass exactly one score source".to_string());
    }
    let transpose = transpose.unwrap_or(0);
    if transpose != 0 && spec.is_some() {
        return Err("--transpose applies to --rtttl/--rtttl-file only".to_string());
    }
    let source = if let Some(spec) = spec {
        ScoreSource::HzMs(spec)
    } else if let Some(rtttl) = rtttl {
        ScoreSource::RtttlText(rtttl)
    } else {
        ScoreSource::RtttlFile(rtttl_file.unwrap_or_default())
    };
    Ok(PreviewCli {
        out,
        source,
        transpose,
    })
}

/// Parses `rtttl_catalogue` arguments without touching the filesystem.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogueCli {
    /// Catalogue path from `--in`.
    pub input: String,
    /// Rust module path from `--out`.
    pub output: String,
    /// Whole-score semitone shift from `--transpose` (every note, all scores).
    pub transpose: i32,
}

/// Parses `rtttl_catalogue --in <catalogue> --out <module.rs> [--transpose N]`.
pub fn parse_catalogue_cli(args: &[String]) -> Result<CatalogueCli, String> {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut transpose: Option<i32> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--in" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --in".to_string());
                }
                input = Some(args[index].clone());
            }
            "--out" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --out".to_string());
                }
                output = Some(args[index].clone());
            }
            "--transpose" => {
                index += 1;
                if index >= args.len() {
                    return Err("missing value for --transpose".to_string());
                }
                match args[index].parse::<i32>() {
                    Ok(shift) => transpose = Some(shift),
                    Err(_) => {
                        return Err(format!("bad --transpose {:?}", args[index]));
                    }
                }
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
        index += 1;
    }
    match (input, output) {
        (Some(input), Some(output)) => Ok(CatalogueCli {
            input,
            output,
            transpose: transpose.unwrap_or(0),
        }),
        _ => Err(
            "usage: rtttl_catalogue --in <catalogue.rtttl> --out <module.rs> [--transpose N]"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests;

pub mod catalogue;
pub mod rtttl;

#[cfg(test)]
mod catalogue_tests;
#[cfg(test)]
mod rtttl_tests;
