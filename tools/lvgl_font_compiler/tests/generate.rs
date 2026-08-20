use std::path::{Path, PathBuf};

use lvgl_font_compiler::{generate_from_path, FontCompilerError, FontSpec};

/// The face the Ambient Home clock is compiled from.
fn vendored_face() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts/IBMPlexSans-Variable.ttf")
}

fn clock_spec(source: &Path) -> FontSpec<'_> {
    FontSpec {
        source,
        size_px: 128.0,
        characters: "0123456789:-",
    }
}

#[test]
fn compiles_the_clock_face() {
    let source = vendored_face();
    let generated = generate_from_path(&clock_spec(&source)).expect("the clock face compiles");

    // 12 characters plus LVGL's reserved "not found" glyph.
    assert!(generated.contains("lv_font_fmt_txt_glyph_dsc_t; 13]"));
    // '-' is on its own; '0'..'9' and ':' are consecutive code points.
    assert!(generated.contains("lv_font_fmt_txt_cmap_t; 2]"));
    assert!(generated.contains("range_start: 45,\n            range_length: 1,"));
    assert!(generated.contains("range_start: 48,\n            range_length: 11,"));
    assert!(
        generated.contains("get_glyph_dsc: Some(::lightvgl_sys::lv_font_get_glyph_dsc_fmt_txt)")
    );
}

#[test]
fn repeated_and_unsorted_characters_do_not_change_the_output() {
    let source = vendored_face();
    let sorted = generate_from_path(&clock_spec(&source)).expect("the clock face compiles");
    let shuffled = generate_from_path(&FontSpec {
        source: &source,
        size_px: 128.0,
        characters: ":9876543210-0",
    })
    .expect("the clock face compiles");

    assert_eq!(sorted, shuffled);
}

#[test]
fn a_character_the_face_lacks_is_reported() {
    let source = vendored_face();
    let error = generate_from_path(&FontSpec {
        source: &source,
        size_px: 128.0,
        characters: "0\u{2603}",
    })
    .expect_err("a snowman is not in IBM Plex Sans");

    assert!(matches!(error, FontCompilerError::MissingGlyph('\u{2603}')));
}

#[test]
fn a_size_that_overflows_the_glyph_fields_is_reported() {
    let source = vendored_face();
    // `adv_w` is 12 bits of 1/16 px, so a 512 px digit cannot be described.
    let error = generate_from_path(&FontSpec {
        source: &source,
        size_px: 512.0,
        characters: "0",
    })
    .expect_err("512 px overflows the LVGL glyph fields");

    assert!(matches!(
        error,
        FontCompilerError::OutOfRange { field: "adv_w", .. }
    ));
}

#[test]
fn an_empty_character_set_is_reported() {
    let source = vendored_face();
    let error = generate_from_path(&FontSpec {
        source: &source,
        size_px: 128.0,
        characters: "",
    })
    .expect_err("an empty face is not compilable");

    assert!(matches!(error, FontCompilerError::NoCharacters));
}

#[test]
fn a_missing_face_is_reported() {
    let source = Path::new("does-not-exist.ttf");
    let error =
        generate_from_path(&clock_spec(source)).expect_err("a missing face cannot be compiled");

    assert!(matches!(error, FontCompilerError::Read(_, _)));
}
