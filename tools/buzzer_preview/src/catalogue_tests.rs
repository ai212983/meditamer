//! Catalogue compiler tests: identifiers, determinism, regeneration.
//!
//! The committed-module check below reads the real catalogue and the real
//! generated file through `CARGO_MANIFEST_DIR`-relative paths, so it proves
//! the working-tree module matches its source without hardcoding either.

use super::catalogue::{
    parse_catalogue, regenerate, render_module, rust_ident_for, source_label_for,
};
use super::{parse_catalogue_cli, parse_preview_cli};

fn manifest_path(relative: &str) -> String {
    format!("{}/{relative}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn identifiers_are_valid_and_collision_safe() {
    assert_eq!(rust_ident_for("ack").unwrap(), "ACK");
    assert_eq!(rust_ident_for("we-ird name!").unwrap(), "WE_IRD_NAME_");
    assert_eq!(rust_ident_for("9lives").unwrap(), "_9LIVES");
    assert_eq!(rust_ident_for("match").unwrap(), "MATCH_");
    assert_eq!(rust_ident_for("catalogue").unwrap(), "CATALOGUE_");
    assert!(rust_ident_for("!!!").is_err());
    let collision = "a-b:d=4,o=6,b=120:c\na b:d=4,o=6,b=120:c\n";
    let error = parse_catalogue(collision, 0).unwrap_err().to_string();
    assert!(error.contains("collides"), "got: {error}");
}

#[test]
fn catalogue_parsing_skips_blanks_and_comments() {
    let text = "# comment\n\nack:d=16,o=6,b=120:c,32p,e\n   \n# tail\n";
    let entries = parse_catalogue(text, 0).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].ident, "ACK");
    assert!(parse_catalogue("# only comments\n\n", 0).is_err());
    let bad_line = "ack:d=16,o=6,b=120:c,32p,e\nbroken:d=4:c,\n";
    let error = parse_catalogue(bad_line, 0).unwrap_err().to_string();
    assert!(error.contains("line 2"), "got: {error}");
}

#[test]
fn generation_is_deterministic() {
    let text = "ack:d=16,o=6,b=120:c,32p,e\ncomplete:d=16,o=6,b=120:c,32p,e,32p,g\n";
    let first = render_module(&parse_catalogue(text, 0).unwrap(), "notes.rtttl", 0);
    let second = render_module(&parse_catalogue(text, 0).unwrap(), "notes.rtttl", 0);
    assert_eq!(first, second);
    assert!(first.contains("pub static ACK: [Element; 3]"));
    assert!(first.contains("pub static COMPLETE: [Element; 5]"));
    assert!(first.contains("pub static CATALOGUE: [Notification; 2]"));
    // No absolute local paths leak into generated headers.
    assert!(!first.contains("/Users/"));
    assert!(!first.contains("/tmp/"));
}

#[test]
fn committed_module_matches_source_catalogue() {
    // Both generated banks (notifications and melodies) share one
    // generator; each committed module must match its source catalogue.
    for (catalogue, module, count) in [
        ("notifications.rtttl", "buzzer_notifications.rs", 3),
        ("melodies.rtttl", "buzzer_melodies.rs", 1),
    ] {
        let catalogue_text =
            std::fs::read_to_string(manifest_path(&format!("../../assets/sounds/{catalogue}")))
                .unwrap();
        let generated = std::fs::read_to_string(manifest_path(&format!(
            "../../targets/meditamer-inkplate/src/{module}"
        )))
        .unwrap();
        let entries = parse_catalogue(&catalogue_text, 0).unwrap();
        assert_eq!(entries.len(), count, "bank {catalogue}");
        assert_eq!(render_module(&entries, catalogue, 0), generated);
    }
}

#[test]
fn failed_regeneration_preserves_output() {
    let dir = std::env::temp_dir().join(format!("rtttl-catalogue-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("bad.rtttl");
    let output = dir.join("module.rs");
    std::fs::write(&input, "ack:d=16,o=6,b=120:c,32p,e\nbroken:d=4:c,\n").unwrap();
    std::fs::write(&output, "// sentinel working module\n").unwrap();
    let result = regenerate(input.to_str().unwrap(), output.to_str().unwrap(), 0);
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(&output).unwrap(),
        "// sentinel working module\n"
    );
    // A valid catalogue through the same path writes the rendered module.
    std::fs::write(&input, "ack:d=16,o=6,b=120:c,32p,e\n").unwrap();
    let summary = regenerate(input.to_str().unwrap(), output.to_str().unwrap(), 0).unwrap();
    assert!(summary.contains("entries=1"), "got: {summary}");
    let written = std::fs::read_to_string(&output).unwrap();
    assert!(written.contains("pub static ACK: [Element; 3]"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn source_labels_stay_relative() {
    assert_eq!(source_label_for("/a/b/notes.rtttl"), "notes.rtttl");
    assert_eq!(source_label_for("notes.rtttl"), "notes.rtttl");
    assert_eq!(source_label_for("a\\b.rtttl"), "b.rtttl");
}

#[test]
fn preview_cli_parsing_covers_new_flags() {
    let args = [
        "--out".to_string(),
        "fixtures/a.wav".to_string(),
        "880:90".to_string(),
    ];
    let cli = parse_preview_cli(&args).unwrap();
    assert_eq!(cli.transpose, 0);
    let args = [
        "--out".to_string(),
        "fixtures/a.wav".to_string(),
        "--rtttl".to_string(),
        "ack:d=16,o=6,b=120:c,32p,e".to_string(),
        "--transpose".to_string(),
        "-2".to_string(),
    ];
    let cli = parse_preview_cli(&args).unwrap();
    assert_eq!(cli.transpose, -2);
    let invalid: &[&[&str]] = &[
        &["--out", "fixtures/a.wav", "--bogus"],
        &["880:90"],
        &["--out", "fixtures/a.wav"],
        &["--out", "fixtures/a.wav", "880:90", "--rtttl", "t:d=4:c"],
        &[
            "--out",
            "fixtures/a.wav",
            "--rtttl",
            "t:d=4:c",
            "--rtttl-file",
            "x",
        ],
        &["--out", "fixtures/a.wav", "880:90", "--transpose", "2"],
        &[
            "--out",
            "fixtures/a.wav",
            "--rtttl",
            "t:d=4:c",
            "--transpose",
            "high",
        ],
        &["--out"],
        &["--out", "fixtures/a.wav", "880:90", "440:90"],
    ];
    for argv in invalid {
        let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        assert!(parse_preview_cli(&owned).is_err(), "should reject {argv:?}");
    }
}

#[test]
fn catalogue_cli_parsing_rejects_bad_flags() {
    let args = [
        "--in".to_string(),
        "a".to_string(),
        "--out".to_string(),
        "b".to_string(),
    ];
    let cli = parse_catalogue_cli(&args).unwrap();
    assert_eq!(cli.input, "a");
    assert_eq!(cli.output, "b");
    assert_eq!(cli.transpose, 0);
    let shifted = [
        "--in".to_string(),
        "a".to_string(),
        "--out".to_string(),
        "b".to_string(),
        "--transpose".to_string(),
        "-3".to_string(),
    ];
    assert_eq!(parse_catalogue_cli(&shifted).unwrap().transpose, -3);
    let invalid: &[&[&str]] = &[
        &["--in", "a"],
        &["--out", "b"],
        &[],
        &["positional"],
        &["--in", "a", "--out", "b", "--extra"],
        &["--in", "a", "--out", "b", "--transpose"],
        &["--in", "a", "--out", "b", "--transpose", "high"],
        &["--in", "a", "--out", "b", "--transpose", "1.5"],
    ];
    for argv in invalid {
        let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        assert!(
            parse_catalogue_cli(&owned).is_err(),
            "should reject {argv:?}"
        );
    }
}

#[test]
fn catalogue_transpose_shifts_every_score_together() {
    // C5 (about 523 Hz) sits below the nominal range, so the unshifted
    // catalogue fails; +12 semitones lands exactly on the C6/E6 score.
    let low = "low:d=16,o=5,b=120:c,e\n";
    let error = parse_catalogue(low, 0).unwrap_err().to_string();
    assert!(error.contains("line 1"), "got: {error}");
    let shifted = parse_catalogue(low, 12).unwrap();
    let explicit = parse_catalogue("low:d=16,o=6,b=120:c,e\n", 0).unwrap();
    assert_eq!(shifted[0].elements, explicit[0].elements);
    assert_eq!(
        buzzer::score::total_duration_ms(&shifted[0].elements),
        buzzer::score::total_duration_ms(&explicit[0].elements)
    );
    // A negative shift that pushes any note out of range fails the whole
    // catalogue, never one note alone.
    let down = parse_catalogue("low:d=16,o=6,b=120:c,e\n", -12)
        .unwrap_err()
        .to_string();
    assert!(down.contains("line 1"), "got: {down}");
    // Absurd and overflowing shifts are checked errors, never wraps.
    assert!(parse_catalogue("low:d=16,o=6,b=120:c\n", i32::MAX).is_err());
    assert!(parse_catalogue("low:d=16,o=6,b=120:c\n", i32::MIN).is_err());
    // G7 pushed up 12 semitones leaves the nominal range.
    assert!(parse_catalogue("hi:d=4,o=7,b=120:g\n", 12).is_err());
    // Provenance records the applied shift while timing is untouched.
    let rendered = render_module(&shifted, "notes.rtttl", 12);
    assert!(rendered.contains("transpose +12"), "got provenance");
    assert!(rendered.contains("pub const COMPILER_TRANSPOSE: i32 = 12;"));
    let plain = render_module(&explicit, "notes.rtttl", 0);
    assert!(plain.contains("transpose +0"), "got provenance");
}

#[test]
fn failed_transposed_regeneration_preserves_output() {
    let dir =
        std::env::temp_dir().join(format!("rtttl-catalogue-transpose-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("notes.rtttl");
    let output = dir.join("module.rs");
    // C5/E5 validates at +12 but not at 0: a failing shift must leave the
    // previous working module untouched.
    std::fs::write(&input, "low:d=16,o=5,b=120:c,e\n").unwrap();
    let summary = regenerate(input.to_str().unwrap(), output.to_str().unwrap(), 12).unwrap();
    assert!(summary.contains("entries=1"), "got: {summary}");
    assert!(summary.contains("transpose=+12"), "got: {summary}");
    let working = std::fs::read_to_string(&output).unwrap();
    assert!(working.contains("pub const COMPILER_TRANSPOSE: i32 = 12;"));
    let result = regenerate(input.to_str().unwrap(), output.to_str().unwrap(), 0);
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), working);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn hostile_source_label_renders_safe_one_comment_line() {
    let label = source_label_for("fixtures/evil\n//! injected\nstatic X: u8 = 1;.rtttl");
    assert!(!label.contains('\n'), "got: {label:?}");
    assert!(!label.contains('\r'), "got: {label:?}");
    assert!(!label.contains('/'), "got: {label:?}");
    let entries = parse_catalogue("ack:d=16,o=6,b=120:c,32p,e\n", 0).unwrap();
    let module = render_module(&entries, &label, 0);
    let header: Vec<&str> = module
        .lines()
        .filter(|line| line.contains("Generated from"))
        .collect();
    assert_eq!(header.len(), 1, "got:\n{module}");
    assert!(header[0].starts_with("//!"), "got: {:?}", header[0]);
}
