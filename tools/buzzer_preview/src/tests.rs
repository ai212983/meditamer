//! Previewer tests: parsing, deterministic rendering, WAV structure.
extern crate std;

use super::*;
use buzzer::score::{Element, Note};

fn note_hz(hz: u32, ms: u32) -> Element {
    Element::Note(Note {
        code: nearest_code(&INKPLATE_OSCILLATOR, hz),
        duration_ms: ms,
    })
}

fn render(elements: &[Element]) -> (Vec<i16>, Rendered) {
    let total_ms = buzzer::score::total_duration_ms(elements);
    let mut samples = vec![0i16; total_ms as usize * SAMPLE_RATE_HZ as usize / 1000];
    let mut actions = vec![
        RequestedAction {
            at_ms: 0,
            kind: ActionKind::PowerOff,
        };
        elements.len() + 1
    ];
    let rendered = render_score(elements, &mut samples, &mut actions);
    samples.truncate(rendered.samples);
    (samples, rendered)
}

#[test]
fn score_spec_parses_notes_rests_and_rejects_garbage() {
    let elements = parse_score_arg("880:90, 0:40 1175:110").unwrap();
    assert_eq!(elements.len(), 3);
    assert!(matches!(elements[1], Element::Rest { duration_ms: 40 }));
    assert!(matches!(elements[0], Element::Note(_)));
    assert!(parse_score_arg("").is_err());
    assert!(parse_score_arg("880").is_err());
    assert!(parse_score_arg("880:0").is_err());
    assert!(parse_score_arg("-440:50").is_err());
    assert!(parse_score_arg("abc:50").is_err());
    // Out-of-range saturates to the endpoint code, exactly as on device.
    let high = parse_score_arg("9999:10").unwrap();
    assert_eq!(
        high,
        std::vec![Element::Note(Note {
            code: 0,
            duration_ms: 10,
        })]
    );
}

#[test]
fn render_is_deterministic_and_square_only() {
    let score = std::vec![note_hz(880, 90), Element::Rest { duration_ms: 40 }];
    let (first, rendered) = render(&score);
    let (second, _) = render(&score);
    assert_eq!(first, second);
    assert_eq!(rendered.total_ms, 130);
    assert_eq!(rendered.actions, 3);
    assert_eq!(first.len(), 130 * SAMPLE_RATE_HZ as usize / 1000);
    // Nominal square wave: full-scale positive, full-scale negative, or
    // silence. No intermediate levels, no invented filtering.
    assert!(first
        .iter()
        .all(|s| *s == AMPLITUDE || *s == -AMPLITUDE || *s == 0));
    assert!(first.contains(&AMPLITUDE));
    assert!(first.contains(&-AMPLITUDE));
    // The rest renders silence from the first sample at or past its
    // start millisecond (sample times quantize below millisecond grid).
    let rest_start = (90 * SAMPLE_RATE_HZ as usize).div_ceil(1000);
    assert!(first[rest_start..].iter().all(|s| *s == 0));
}

#[test]
fn power_on_sounds_default_code_over_startup() {
    // A 2000 Hz request maps near 1982 Hz (about four zero crossings per
    // millisecond), while the default-code blip runs near 993 Hz (about two
    // per millisecond starting from a fresh phase). The first millisecond
    // must show the slow blip, and the programmed pitch must take over only
    // once the startup wait elapses.
    let score = std::vec![note_hz(2000, 20)];
    let (samples, _) = render(&score);
    let ms = SAMPLE_RATE_HZ as usize / 1000;
    // Fresh-phase ~993 Hz blip: positive for the first half period (about
    // eleven samples), then oscillating. A programmed pitch sounding from
    // power-on would already have flipped by sample six; silence would show
    // no negative samples at all.
    let blip = &samples[..ms];
    assert!(blip[..11].iter().all(|s| *s == AMPLITUDE));
    assert!(blip.contains(&-AMPLITUDE));
    // Sixteen milliseconds well inside the programmed pitch: ~1982 Hz
    // crosses about 63 times, while a stuck startup blip near 993 Hz would
    // cross about 32 times and silence none. The band is wide enough to be
    // phase-independent and narrow enough to catch any of those faults.
    let window = &samples[2 * ms..18 * ms];
    assert!(window.contains(&AMPLITUDE));
    assert!(window.contains(&-AMPLITUDE));
    let mut crossings = 0u32;
    for pair in window.windows(2) {
        if (pair[0] > 0) != (pair[1] > 0) {
            crossings += 1;
        }
    }
    assert!(
        (50..=75).contains(&crossings),
        "programmed pitch crossings {crossings}, expected a ~1982 Hz square wave"
    );
}

#[test]
fn leading_rest_renders_silence_then_audible_pitch() {
    // `0:20 2000:20`: the cursor must apply the delayed initial action
    // instead of assuming the first action sits at 0.
    let score = std::vec![Element::Rest { duration_ms: 20 }, note_hz(2000, 20),];
    let (samples, rendered) = render(&score);
    assert_eq!(rendered.total_ms, 40);
    assert_eq!(rendered.actions, 2);
    let ms = SAMPLE_RATE_HZ as usize / 1000;
    assert!(samples[..20 * ms].iter().all(|s| *s == 0));
    let tail = &samples[20 * ms..];
    assert!(tail.contains(&AMPLITUDE));
    assert!(tail.contains(&-AMPLITUDE));
}

#[test]
fn trailing_rest_renders_silence_to_score_end() {
    let score = std::vec![note_hz(880, 20), Element::Rest { duration_ms: 20 }];
    let (samples, rendered) = render(&score);
    assert_eq!(rendered.total_ms, 40);
    let ms = SAMPLE_RATE_HZ as usize / 1000;
    assert!(samples[..ms].contains(&AMPLITUDE));
    let rest_start = (20 * SAMPLE_RATE_HZ as usize).div_ceil(1000);
    assert!(samples[rest_start..].iter().all(|s| *s == 0));
}

#[test]
fn zero_duration_elements_leave_no_trace() {
    let score = std::vec![
        Element::Note(Note {
            code: 60,
            duration_ms: 0,
        }),
        Element::Rest { duration_ms: 0 },
        note_hz(880, 20),
    ];
    let (samples, rendered) = render(&score);
    assert_eq!(rendered.total_ms, 20);
    assert_eq!(rendered.actions, 2);
    assert!(samples.contains(&AMPLITUDE));
    assert!(samples.contains(&-AMPLITUDE));
}

#[test]
fn wav_header_describes_pcm16_mono() {
    let wav = write_wav(&[AMPLITUDE, -AMPLITUDE, 0], SAMPLE_RATE_HZ);
    assert_eq!(wav.len(), 44 + 6);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    assert_eq!(&wav[12..16], b"fmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1);
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
    assert_eq!(
        u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
        SAMPLE_RATE_HZ
    );
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6);
    assert_eq!(&wav[44..46], AMPLITUDE.to_le_bytes());
}
