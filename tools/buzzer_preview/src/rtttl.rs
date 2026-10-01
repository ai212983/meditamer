//! Host-only RTTTL frontend for the shared buzzer sequencer.
//!
//! Compiles `name:defaults:notes` text into borrowed-score-compatible
//! [`Element`] vectors. The board-specific pitch mapping already lives in
//! the shared model; this module only parses RTTTL and quantizes through
//! [`INKPLATE_OSCILLATOR`] and [`nearest_code`]. There is no firmware
//! parser: RTTTL never reaches the device.
//!
//! Grammar (audited against the ESPHome RTTTL documentation and its
//! `rtttl.cpp` parser; no third-party code is copied):
//!
//! ```text
//! rtttl   = name ":" defaults ":" notes
//! defaults = [key "=" value ("," key "=" value)*]   // order independent
//! key      = "d" | "o" | "b"                        // no duplicates, no unknown keys
//! notes    = note ("," note)*                       // no empty body, no trailing comma
//! note     = [duration] pitch ["#"] ["."] [octave] ["."]
//! duration = "1" | "2" | "4" | "8" | "16" | "32"
//! pitch    = "c" | "d" | "e" | "f" | "g" | "a" | "b" | "p"  // case-insensitive
//! octave   = "4" | "5" | "6" | "7"
//! ```
//!
//! Defaults are `d=4, o=6, b=63` when a key is absent. At most one dot per
//! note lengthens it by half; the dot may sit before or after an explicit
//! octave (`c#.6` and `c#6.` both parse) for compatibility with RTTTL
//! sources that disagree on dot placement. A dot on a rest (`32p.`) is
//! accepted the same way. Sharps (`#`) apply to pitches only, never `p`.
//!
//! Octave convention: `C4` is MIDI 60 at about 261.63 Hz and `A4` is MIDI 69
//! at exactly 440 Hz; frequency is `440 * 2^((midi - 69) / 12)`.
//!
//! Pitch strictness: a request is checked as an unrounded equal-temperament
//! frequency against the true nominal endpoints (`code 0` and `code 127`
//! frequencies, not their truncated reports). Anything outside is rejected
//! with the offending note identified; there is no silent clamping and no
//! per-note fallback transpose.
//!
//! Transpose is an explicit signed whole-score semitone shift: every note's
//! MIDI number moves together, rests and durations are untouched. A shift
//! that pushes any note out of range fails the whole score with that note
//! identified; no note is clamped or shifted individually.
//!
//! Timing uses cumulative rounding so fractional beats never drift: with
//! `whole_ms = 240000 / tempo_bpm`, boundary `i` lands at
//! `round_half_up(cum_64ths_i * 60000 / (64 * tempo_bpm))` where `cum_64ths`
//! is the exact cumulative length in 64th-beats (a 32nd is 8 units, dotted
//! multiplies by 3/2 exactly), and each element sounds the difference of
//! successive boundaries. The total is the final boundary.
//!
//! Articulation: adjacent notes are preserved literally as adjacent
//! [`Element::Note`] values, which the shared planner renders as one
//! continuous powered run (`SetCode` pitch writes, no power cycle). Authors
//! who want separate beeps must write `p` rests between notes; this
//! compiler never inserts hidden gaps.
//!
//! Bounds (short-notification envelope): at most [`MAX_TOKENS`] notes/rests
//! per score and at most [`MAX_TOTAL_MS`] milliseconds total. All numeric
//! input arithmetic is checked; oversized or negative inputs are errors,
//! never wraps or silent saturation.
//!
//! [`Element`]: buzzer::score::Element
//! [`INKPLATE_OSCILLATOR`]: super::INKPLATE_OSCILLATOR
//! [`nearest_code`]: buzzer::model::nearest_code

use buzzer::model::{nearest_code, ASTABLE_NUM_HZ_OHM};
use buzzer::score::{Element, Note};

use super::INKPLATE_OSCILLATOR;

/// Maximum notes plus rests per compiled score.
pub const MAX_TOKENS: usize = 256;

/// Maximum total score duration in milliseconds.
pub const MAX_TOTAL_MS: u64 = 30_000;

/// Duration used when the header omits `d`.
pub const DEFAULT_DURATION: u32 = 4;

/// Octave used when the header omits `o`.
pub const DEFAULT_OCTAVE: u8 = 6;

/// Tempo in beats per minute used when the header omits `b`.
pub const DEFAULT_TEMPO_BPM: u32 = 63;

/// Smallest accepted tempo in beats per minute (must stay positive).
pub const MIN_TEMPO_BPM: u32 = 1;

/// Largest accepted tempo in beats per minute (bounds pathological timing).
pub const MAX_TEMPO_BPM: u32 = 1024;

/// Smallest accepted per-note octave.
pub const MIN_OCTAVE: u8 = 4;

/// Largest accepted per-note octave.
pub const MAX_OCTAVE: u8 = 7;

/// One RTTTL compilation failure, naming the offending input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(pub String);

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

/// Nominal endpoint resistance span in ohms: code 0 (highest pitch) uses
/// only the base resistance; code 127 (lowest pitch) adds full RAB.
fn endpoint_freq(open: bool) -> f64 {
    let resistance = if open {
        u64::from(INKPLATE_OSCILLATOR.r_base_ohm)
    } else {
        u64::from(INKPLATE_OSCILLATOR.r_base_ohm) + u64::from(INKPLATE_OSCILLATOR.r_pot_ohm)
    };
    ASTABLE_NUM_HZ_OHM as f64 / resistance as f64
}

/// Highest nominal pitch (code 0), unrounded.
pub fn nominal_max_hz() -> f64 {
    endpoint_freq(true)
}

/// Lowest nominal pitch (code 127), unrounded.
pub fn nominal_min_hz() -> f64 {
    endpoint_freq(false)
}

/// Equal-temperament frequency in Hz for a MIDI number (`A4` = 69 = 440 Hz).
pub fn midi_freq_hz(midi: i32) -> f64 {
    440.0 * 2.0f64.powf((f64::from(midi) - 69.0) / 12.0)
}

fn semitone_offset(pitch: char) -> Option<i32> {
    match pitch {
        'c' => Some(0),
        'd' => Some(2),
        'e' => Some(4),
        'f' => Some(5),
        'g' => Some(7),
        'a' => Some(9),
        'b' => Some(11),
        _ => None,
    }
}

const VALID_DURATIONS: [u32; 6] = [1, 2, 4, 8, 16, 32];

struct Defaults {
    duration: u32,
    octave: u8,
    tempo_bpm: u32,
}

fn parse_header_value(text: &str, what: &str) -> Result<u32, Error> {
    let digits = text.trim();
    if digits.is_empty() {
        return fail(format!("empty header value for {what}"));
    }
    if digits.starts_with(['+', '-']) {
        return fail(format!("invalid header value {text:?} for {what}"));
    }
    match digits.parse::<u32>() {
        Ok(value) => Ok(value),
        Err(_) => fail(format!("invalid header value {text:?} for {what}")),
    }
}

fn parse_defaults(text: &str) -> Result<Defaults, Error> {
    let mut duration = DEFAULT_DURATION;
    let mut octave = DEFAULT_OCTAVE;
    let mut tempo_bpm = DEFAULT_TEMPO_BPM;
    let mut seen_d = false;
    let mut seen_o = false;
    let mut seen_b = false;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Defaults {
            duration,
            octave,
            tempo_bpm,
        });
    }
    for item in trimmed.split(',') {
        let item = item.trim();
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| Error(format!("malformed header item {item:?}")))?;
        let key = key.trim().to_ascii_lowercase();
        match key.as_str() {
            "d" | "o" | "b" => {}
            _ => return fail(format!("unknown header key {key:?}")),
        }
        let value = parse_header_value(value, key.as_str())?;
        match key.as_str() {
            "d" => {
                if seen_d {
                    return fail("duplicate header key 'd'");
                }
                seen_d = true;
                if !VALID_DURATIONS.contains(&value) {
                    return fail(format!("invalid default duration {value}"));
                }
                duration = value;
            }
            "o" => {
                if seen_o {
                    return fail("duplicate header key 'o'");
                }
                seen_o = true;
                if !(u32::from(MIN_OCTAVE)..=u32::from(MAX_OCTAVE)).contains(&value) {
                    return fail(format!("invalid default octave {value}"));
                }
                octave = value as u8;
            }
            "b" => {
                if seen_b {
                    return fail("duplicate header key 'b'");
                }
                seen_b = true;
                if !(MIN_TEMPO_BPM..=MAX_TEMPO_BPM).contains(&value) {
                    return fail(format!("invalid tempo {value}"));
                }
                tempo_bpm = value;
            }
            _ => return fail(format!("unknown header key {key:?}")),
        }
    }
    Ok(Defaults {
        duration,
        octave,
        tempo_bpm,
    })
}

struct ParsedNote {
    rest: bool,
    midi: i32,
    units_64ths: u64,
    token: String,
}

fn parse_token(token: &str, defaults: &Defaults) -> Result<ParsedNote, Error> {
    let bad = || Error(format!("malformed note token {token:?}"));
    let bytes = token.as_bytes();
    let mut pos = 0;
    // Optional duration prefix: one of 1,2,4,8,16,32.
    let mut duration = defaults.duration;
    if pos < bytes.len() && bytes[pos].is_ascii_digit() {
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        let value: u32 = token[start..pos].parse().map_err(|_| bad())?;
        if !VALID_DURATIONS.contains(&value) {
            return fail(format!("invalid duration in token {token:?}"));
        }
        duration = value;
    }
    // Pitch letter.
    if pos >= bytes.len() {
        return Err(bad());
    }
    let pitch = (bytes[pos] as char).to_ascii_lowercase();
    if !"cdefgabp".contains(pitch) {
        return Err(bad());
    }
    let rest = pitch == 'p';
    pos += 1;
    // Optional sharp (pitches only).
    let mut sharp = false;
    if pos < bytes.len() && bytes[pos] == b'#' {
        if rest {
            return fail(format!("sharp rest in token {token:?}"));
        }
        sharp = true;
        pos += 1;
    }
    // Optional dot before the octave.
    let mut dotted = false;
    if pos < bytes.len() && bytes[pos] == b'.' {
        dotted = true;
        pos += 1;
    }
    // Optional explicit octave (rests take none).
    let mut octave = defaults.octave;
    if pos < bytes.len() && (bytes[pos] as char).is_ascii_digit() {
        if rest {
            return fail(format!("octave on rest in token {token:?}"));
        }
        let value = u32::from(bytes[pos] - b'0');
        if !(u32::from(MIN_OCTAVE)..=u32::from(MAX_OCTAVE)).contains(&value) {
            return fail(format!("invalid octave in token {token:?}"));
        }
        octave = value as u8;
        pos += 1;
    }
    // Optional dot after the octave (exactly one dot total).
    if pos < bytes.len() && bytes[pos] == b'.' {
        if dotted {
            return fail(format!("double dot in token {token:?}"));
        }
        dotted = true;
        pos += 1;
    }
    if pos != bytes.len() {
        return Err(bad());
    }
    // Length in 64th-beats: a 32nd is 8 units, dotted multiplies by 3/2.
    let mut units = 256u64 / u64::from(duration);
    if dotted {
        units = units
            .checked_mul(3)
            .and_then(|tripled| tripled.checked_div(2))
            .ok_or_else(|| Error(format!("duration overflow in token {token:?}")))?;
    }
    let midi = if rest {
        0
    } else {
        let offset = semitone_offset(pitch).ok_or_else(bad)?;
        let sharp_step = i32::from(sharp);
        12 * (i32::from(octave) + 1) + offset + sharp_step
    };
    Ok(ParsedNote {
        rest,
        midi,
        units_64ths: units,
        token: token.to_string(),
    })
}

/// Compile RTTTL text to score elements, applying a whole-score semitone
/// transpose (`0` for none). See the module docs for grammar, strictness,
/// rounding, articulation, and bounds.
pub fn compile(text: &str, transpose: i32) -> Result<Vec<Element>, Error> {
    let mut sections = text.split(':');
    let name = sections.next().unwrap_or("").trim();
    let defaults_text = sections
        .next()
        .ok_or_else(|| Error("missing defaults section".into()))?;
    let body = sections
        .next()
        .ok_or_else(|| Error("missing notes section".into()))?;
    if sections.next().is_some() {
        return fail("extra section after notes (expected name:defaults:notes)");
    }
    if name.is_empty() {
        return fail("empty score name");
    }
    let defaults = parse_defaults(defaults_text)?;
    if body.trim().is_empty() {
        return fail("empty note body");
    }
    let mut notes = Vec::new();
    for raw in body.split(',') {
        let token = raw.trim();
        if token.is_empty() {
            return fail("empty note token (trailing or doubled comma?)");
        }
        if notes.len() >= MAX_TOKENS {
            return fail(format!("too many notes (max {MAX_TOKENS})"));
        }
        notes.push(parse_token(token, &defaults)?);
    }
    if notes.is_empty() {
        return fail("empty note body");
    }
    // Cumulative exact timing in 64th-beats; boundaries round half-up to
    // whole milliseconds so fractional beats never accumulate drift.
    let tempo = u64::from(defaults.tempo_bpm);
    let mut cumulative_units = 0u64;
    let mut boundaries = Vec::with_capacity(notes.len() + 1);
    boundaries.push(0u64);
    for note in &notes {
        cumulative_units = cumulative_units
            .checked_add(note.units_64ths)
            .ok_or_else(|| Error("score length overflow".into()))?;
        let numerator = cumulative_units
            .checked_mul(60_000)
            .ok_or_else(|| Error("score length overflow".into()))?;
        let denominator = 64u64
            .checked_mul(tempo)
            .ok_or_else(|| Error("score length overflow".into()))?;
        // Round half-up: (n + d/2) / d.
        let boundary = numerator
            .checked_add(denominator / 2)
            .and_then(|rounded| rounded.checked_div(denominator))
            .ok_or_else(|| Error("score length overflow".into()))?;
        boundaries.push(boundary);
    }
    let total_ms = boundaries[notes.len()];
    if total_ms > MAX_TOTAL_MS {
        return fail(format!(
            "score too long ({total_ms} ms, max {MAX_TOTAL_MS} ms)"
        ));
    }
    let min_hz = nominal_min_hz();
    let max_hz = nominal_max_hz();
    let mut elements = Vec::with_capacity(notes.len());
    for (index, note) in notes.iter().enumerate() {
        let duration_ms = boundaries[index + 1]
            .checked_sub(boundaries[index])
            .ok_or_else(|| Error("score length overflow".into()))?;
        let duration_ms =
            u32::try_from(duration_ms).map_err(|_| Error("score length overflow".into()))?;
        if note.rest {
            elements.push(Element::Rest { duration_ms });
            continue;
        }
        let shifted = note.midi.checked_add(transpose).ok_or_else(|| {
            Error(format!(
                "note {} ({:?}) transposes out of range",
                index + 1,
                note.token
            ))
        })?;
        if !(0..=127).contains(&shifted) {
            return fail(format!(
                "note {} ({:?}) transposes out of MIDI range",
                index + 1,
                note.token
            ));
        }
        let freq = midi_freq_hz(shifted);
        if freq < min_hz || freq > max_hz {
            return fail(format!(
                "note {} ({:?}) at {freq:.2} Hz is outside the nominal {:.2}..{:.2} Hz range; transpose the whole score instead",
                index + 1,
                note.token,
                min_hz,
                max_hz
            ));
        }
        // Tested range above, so the truncation below cannot wrap.
        let code = nearest_code(&INKPLATE_OSCILLATOR, freq as u32);
        elements.push(Element::Note(Note { code, duration_ms }));
    }
    Ok(elements)
}
