//! RTTTL frontend tests: grammar, strictness, timing, and limits.
//!
//! Expectations below come from hand computation (equal temperament,
//! cumulative rounding) and the documented nominal mapping, not from
//! re-implementing the compiler.

use buzzer::score::{plan, ActionKind, Element};

use super::rtttl::{
    compile, midi_freq_hz, nominal_max_hz, nominal_min_hz, MAX_TOKENS, MAX_TOTAL_MS,
};

fn codes(elements: &[Element]) -> Vec<Option<u8>> {
    elements
        .iter()
        .map(|element| match *element {
            Element::Note(note) => Some(note.code),
            Element::Rest { .. } => None,
        })
        .collect()
}

fn durations(elements: &[Element]) -> Vec<u32> {
    elements
        .iter()
        .map(|element| match *element {
            Element::Note(note) => note.duration_ms,
            Element::Rest { duration_ms } => duration_ms,
        })
        .collect()
}

#[test]
fn octave_convention_matches_a440() {
    assert_eq!(midi_freq_hz(69), 440.0);
    let c4 = midi_freq_hz(60);
    assert!(
        (c4 - 261.63).abs() < 0.01,
        "C4 should be about 261.63 Hz, got {c4}"
    );
}

#[test]
fn nominal_endpoints_are_unrounded_quotients() {
    assert_eq!(nominal_max_hz(), 7_213_000.0 / 2_300.0);
    assert_eq!(nominal_min_hz(), 7_213_000.0 / 12_300.0);
}

#[test]
fn defaults_apply_and_overrides_win() {
    // Empty defaults section: d=4, o=6, b=63. A quarter at 63 bpm:
    // 60000/63 = 952.38 ms rounds to 952.
    let defaulted = compile("t::c", 0).unwrap();
    assert_eq!(codes(&defaulted), [Some(58)]);
    assert_eq!(durations(&defaulted), [952]);
    // Header order is independent and per-note values override defaults.
    let reordered = compile("t:b=120,o=6,d=16:c", 0).unwrap();
    let nominal = compile("t:d=16,o=6,b=120:c", 0).unwrap();
    assert_eq!(reordered, nominal);
    let over = compile("t:d=16,o=6,b=120:4c6,8e7", 0).unwrap();
    assert_eq!(durations(&over), [500, 250]);
}

#[test]
fn sharps_dots_and_rests() {
    // C#6 vs C6 differ; a dotted quarter at 60 bpm is 1500 ms.
    let sharp = compile("t:d=4,o=6,b=60:c#", 0).unwrap();
    let plain = compile("t:d=4,o=6,b=60:c", 0).unwrap();
    assert_ne!(codes(&sharp), codes(&plain));
    let dotted = compile("t:d=4,o=6,b=60:c.", 0).unwrap();
    assert_eq!(durations(&dotted), [1500]);
    let rest = compile("t:d=4,o=6,b=60:8p", 0).unwrap();
    assert!(matches!(rest[..], [Element::Rest { duration_ms: 500 }]));
    // Dot before or after the octave parses identically.
    let before = compile("t:d=16,o=5,b=120:c#.6", 0).unwrap();
    let after = compile("t:d=16,o=5,b=120:c#6.", 0).unwrap();
    assert_eq!(before, after);
    // Case-insensitive pitches and header keys.
    let upper = compile("t:D=16,O=6,B=120:C", 0).unwrap();
    assert_eq!(upper, compile("t:d=16,o=6,b=120:c", 0).unwrap());
}

#[test]
fn malformed_inputs_fail() {
    let bad = [
        "no-colons-here",
        "t:d=4",
        "t:d=4:o=6:c",
        ":d=4:c",
        "t:d=4:",
        "t:d=4:   ",
        "t:d=4:c,",
        "t:d=4:,c",
        "t:d=4,d=8:c",
        "t:o=6,o=6:c",
        "t:b=120,b=120:c",
        "t:x=4:c",
        "t:d:c",
        "t:d=3:c",
        "t:d=0:c",
        "t:o=3:c",
        "t:o=8:c",
        "t:b=0:c",
        "t:b=-5:c",
        "t:b=99999:c",
        "t:d=4:q",
        "t:d=4:c##",
        "t:d=4:c..",
        "t:d=4:4c8",
        "t:d=4:c9",
        "t:d=4:8#",
        "t:d=4:p#",
        "t:d=4:4p6",
        "t:d=4:c3p",
    ];
    for text in bad {
        assert!(compile(text, 0).is_err(), "should reject {text:?}");
    }
}

#[test]
fn motif_examples_compile_to_expected_scores() {
    let ack = compile("ack:d=16,o=6,b=120:c,32p,e", 0).unwrap();
    assert_eq!(codes(&ack), [Some(58), None, Some(40)]);
    assert_eq!(durations(&ack), [125, 63, 125]);
    assert_eq!(buzzer::score::total_duration_ms(&ack), 313);
    let attention = compile("attention:d=16,o=6,b=120:e,32p,e", 0).unwrap();
    assert_eq!(codes(&attention), [Some(40), None, Some(40)]);
    assert_eq!(durations(&attention), [125, 63, 125]);
    let complete = compile("complete:d=16,o=6,b=120:c,32p,e,32p,g", 0).unwrap();
    assert_eq!(codes(&complete), [Some(58), None, Some(40), None, Some(29)]);
    assert_eq!(durations(&complete), [125, 63, 125, 62, 125]);
    assert_eq!(buzzer::score::total_duration_ms(&complete), 500);
}

#[test]
fn cumulative_rounding_avoids_per_note_drift() {
    // Three 32nds at 120 bpm are exactly 62.5 ms each: naive per-note
    // rounding would total 63*3 = 189, but the exact total is 187.5.
    // Cumulative boundaries (62.5->63, 125->125, 187.5->188) keep 188.
    let three = compile("t:d=32,b=120:c,c,c", 0).unwrap();
    assert_eq!(durations(&three), [63, 62, 63]);
    assert_eq!(buzzer::score::total_duration_ms(&three), 188);
}

#[test]
fn out_of_range_notes_name_the_note_and_suggest_transpose() {
    // C4 (261.63 Hz) sits below the nominal 586.42 Hz endpoint.
    let error = compile("t:o=4,b=120:c", 0).unwrap_err().to_string();
    assert!(error.contains("note 1"), "got: {error}");
    assert!(error.contains('c'), "got: {error}");
    assert!(error.contains("transpose"), "got: {error}");
    // A7 (3520 Hz) sits above the nominal 3136.09 Hz endpoint.
    let high = compile("t:d=4,o=7,b=120:a", 0).unwrap_err().to_string();
    assert!(high.contains("note 1"), "got: {high}");
}

#[test]
fn whole_score_transpose_shifts_every_note_together() {
    // +12 semitones on C6 equals writing C7 directly.
    let shifted = compile("t:d=4,o=6,b=120:c,e", 12).unwrap();
    let explicit = compile("t:d=4,o=7,b=120:c,e", 0).unwrap();
    assert_eq!(shifted, explicit);
    // -12 on C6 lands C5 below the range: the whole score fails.
    let down = compile("t:d=4,o=6,b=120:c", -12).unwrap_err().to_string();
    assert!(down.contains("note 1"), "got: {down}");
    // G6 survives +12 (becomes G7) but G7 does not (becomes G8): the
    // surviving note is still rejected with the run, never clamped alone.
    let mixed = compile("t:d=4,o=6,b=120:g6,g7", 12)
        .unwrap_err()
        .to_string();
    assert!(mixed.contains('2'), "got: {mixed}");
    // Absurd shifts are checked errors, never panics or wraps.
    assert!(compile("t:d=4,o=6,b=120:c", i32::MAX).is_err());
    assert!(compile("t:d=4,o=6,b=120:c", i32::MIN).is_err());
}

#[test]
fn notification_limits_hold() {
    // 256 short notes fit the token count (256 * ~7 ms stays under 30 s).
    let token = "c";
    let full_body = std::vec![token; MAX_TOKENS].join(",");
    let full = format!("t:d=32,o=6,b=1024:{full_body}");
    assert_eq!(compile(&full, 0).unwrap().len(), MAX_TOKENS);
    // One more token exceeds the count, even though it would fit in time.
    let over = format!("t:d=32,o=6,b=1024:{full_body},{token}");
    let error = compile(&over, 0).unwrap_err().to_string();
    assert!(error.contains("too many"), "got: {error}");
    // Eight whole notes at 60 bpm are 32 s: over the duration cap.
    let long_body = std::vec!["c"; 8].join(",");
    let long = format!("t:d=1,o=6,b=60:{long_body}");
    let error = compile(&long, 0).unwrap_err().to_string();
    assert!(error.contains("too long"), "got: {error}");
    assert!(error.contains(&MAX_TOTAL_MS.to_string()), "got: {error}");
    // Seven whole notes (28 s) are accepted.
    let ok_body = std::vec!["c"; 7].join(",");
    let ok = format!("t:d=1,o=6,b=60:{ok_body}");
    assert_eq!(
        buzzer::score::total_duration_ms(&compile(&ok, 0).unwrap()),
        28_000
    );
}

#[test]
fn repeated_notes_stay_continuous_without_hidden_gaps() {
    // Adjacent equal codes compile to adjacent notes; the shared planner
    // keeps power on across them (SetCode), exactly as for any pitch
    // change. Separate beeps need explicit `p` rests, which the catalogue
    // motifs already carry.
    let pair = compile("t:d=16,o=6,b=120:e,e", 0).unwrap();
    assert_eq!(codes(&pair), [Some(40), Some(40)]);
    let actions: Vec<_> = plan(&pair).collect();
    assert_eq!(actions.len(), 2);
    assert!(matches!(
        actions[0].kind,
        ActionKind::PowerOnSetCode { code: 40 }
    ));
    assert!(matches!(actions[1].kind, ActionKind::SetCode { code: 40 }));
    // With a rest between, power cycles as usual.
    let split = compile("t:d=16,o=6,b=120:e,32p,e", 0).unwrap();
    let actions: Vec<_> = plan(&split).collect();
    assert_eq!(actions.len(), 3);
    assert!(matches!(actions[1].kind, ActionKind::PowerOff));
}
