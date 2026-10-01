//! Deterministic notification-catalogue compiler.
//!
//! Reads a text catalogue with one RTTTL score per line (blank lines and
//! `#` comment lines skipped), validates every entry with the [`rtttl`]
//! frontend, and renders one self-contained Rust module holding static
//! element arrays plus a small borrowed catalogue. The whole catalogue is
//! validated before anything is rendered, so a failure never produces a
//! half-written module; callers must only write the output file after
//! [`render_module`] succeeds, which keeps a previous working module
//! intact across a failed regeneration.
//!
//! Score names become `UPPER_SNAKE` static identifiers. Names that cannot
//! map to a valid, collision-free Rust identifier fail the catalogue with
//! the line number identified; user text only ever lands in `//` comments
//! and string literals, never in code position.
//!
//! [`rtttl`]: super::rtttl

use buzzer::score::total_duration_ms;

use super::rtttl::{compile, Error};
use super::INKPLATE_OSCILLATOR;

/// One validated catalogue entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// Derived `UPPER_SNAKE` static name.
    pub ident: String,
    /// RTTTL score name.
    pub name: String,
    /// Source RTTTL line (kept for the generated comment only).
    pub rtttl: String,
    /// Compiled elements (whole-catalogue transpose applied at parse time).
    pub elements: Vec<buzzer::score::Element>,
}

/// Rust strict/reserved keywords that a derived identifier must avoid
/// (compared case-insensitively; `self`/`Self` handled by case).
const KEYWORDS: &[&str] = &[
    "AS", "BREAK", "CONST", "CONTINUE", "CRATE", "ELSE", "ENUM", "EXTERN", "FALSE", "FN", "FOR",
    "IF", "IMPL", "IN", "LET", "LOOP", "MATCH", "MOD", "MOVE", "MUT", "PUB", "REF", "RETURN",
    "SELF", "STATIC", "STRUCT", "SUPER", "TRAIT", "TRUE", "TYPE", "UNSAFE", "USE", "WHERE",
    "WHILE", "ASYNC", "AWAIT", "DYN", "ABSTRACT", "BECOME", "BOX", "DO", "FINAL", "MACRO",
    "OVERRIDE", "PRIV", "TYPEOF", "UNSIZED", "VIRTUAL", "YIELD", "TRY", "UNION", "GEN",
];

/// Module-level item names this compiler emits; entries must not collide.
const RESERVED_IDENTS: &[&str] = &["CATALOGUE", "NOTIFICATION", "ELEMENT", "NOTE"];

/// Map an RTTTL score name to a `UPPER_SNAKE` static identifier.
pub fn rust_ident_for(name: &str) -> Result<String, Error> {
    let mut ident = String::with_capacity(name.len());
    let mut alphanumeric = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            alphanumeric = true;
            ident.push(ch.to_ascii_uppercase());
        } else {
            ident.push('_');
        }
    }
    if !alphanumeric {
        return Err(Error(format!(
            "score name {name:?} has no usable identifier characters"
        )));
    }
    if ident.starts_with(|c: char| c.is_ascii_digit()) {
        ident.insert(0, '_');
    }
    if KEYWORDS.contains(&ident.as_str()) || RESERVED_IDENTS.contains(&ident.as_str()) {
        ident.push('_');
    }
    Ok(ident)
}

/// File-name portion of a catalogue path for generated headers: never an
/// absolute local path, so generated files stay portable and diffable.
/// Control characters (including newlines) are replaced with `_` so the
/// generated `//!` header always stays one comment line: a hostile file
/// name cannot inject extra lines or code into the rendered module.
pub fn source_label_for(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let tail = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    let base = if tail.is_empty() { trimmed } else { tail };
    let sanitized: String = base
        .chars()
        .map(|ch| {
            if ch.is_control() || ch == '`' {
                '_'
            } else {
                ch
            }
        })
        .collect();
    if sanitized.is_empty() {
        String::from("catalogue.rtttl")
    } else {
        sanitized
    }
}

/// Parse and validate a whole catalogue, applying one signed whole-score
/// semitone shift to every note of every score through [`compile`]; `0`
/// reproduces the unshifted catalogue. Every line is checked before any
/// code is rendered.
pub fn parse_catalogue(text: &str, transpose: i32) -> Result<Vec<Entry>, Error> {
    let mut entries = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line.split(':').next().unwrap_or("").trim().to_string();
        if name.is_empty() {
            return Err(Error(format!("line {line_number}: empty score name")));
        }
        let elements = compile(line, transpose)
            .map_err(|error| Error(format!("line {line_number}: {error}")))?;
        let ident =
            rust_ident_for(&name).map_err(|error| Error(format!("line {line_number}: {error}")))?;
        if entries.iter().any(|entry: &Entry| entry.ident == ident) {
            return Err(Error(format!(
                "line {line_number}: identifier {ident} collides with an earlier entry"
            )));
        }
        entries.push(Entry {
            ident,
            name,
            rtttl: line.to_string(),
            elements,
        });
    }
    if entries.is_empty() {
        return Err(Error("catalogue holds no scores".into()));
    }
    Ok(entries)
}

/// Read a catalogue file, validate every entry with `transpose` applied,
/// render the module, and write the output file. The output is only
/// touched after the whole catalogue validates, so a failed regeneration
/// preserves the previous working module byte-for-byte. Returns a one-line
/// summary for CLI output.
pub fn regenerate(input_path: &str, output_path: &str, transpose: i32) -> Result<String, String> {
    let text = std::fs::read_to_string(input_path)
        .map_err(|error| format!("cannot read {input_path}: {error}"))?;
    let entries = parse_catalogue(&text, transpose).map_err(|error| error.to_string())?;
    let module = render_module(&entries, &source_label_for(input_path), transpose);
    std::fs::write(output_path, &module)
        .map_err(|error| format!("cannot write {output_path}: {error}"))?;
    let mut total_ms = 0u64;
    for entry in &entries {
        total_ms += total_duration_ms(&entry.elements);
    }
    Ok(format!(
        "CATALOGUE entries={} transpose={transpose:+} total_ms={total_ms} path={output_path}",
        entries.len()
    ))
}

/// Render the validated entries as one deterministic Rust module.
/// `transpose` is provenance only: it records the shift `entries` were
/// compiled with and never changes timing (transpose moves pitch, rests
/// and durations are untouched by [`compile`]).
pub fn render_module(entries: &[Entry], source_label: &str, transpose: i32) -> String {
    let mut out = String::new();
    out.push_str("//! Short notification scores for the Inkplate buzzer probe.\n");
    out.push_str("//!\n");
    out.push_str(&format!(
        "//! Generated from `{source_label}` (transpose {transpose:+}) by the\n"
    ));
    out.push_str("//! `rtttl_catalogue` host tool; see\n");
    out.push_str(
        "//! `docs/guides/audio/buzzer-listening.md` for the regenerate command. Do not\n",
    );
    out.push_str("//! edit: change the catalogue and regenerate. These are listening\n");
    out.push_str("//! candidates, not acoustically selected presets.\n");
    out.push_str("use buzzer::score::{Element, Note};\n");
    out.push('\n');
    out.push_str("/// Oscillator model the host compiler quantized through, as\n");
    out.push_str("/// read-only provenance for the firmware guard: the probe compares\n");
    out.push_str("/// these against the board `OSCILLATOR` and refuses on mismatch.\n");
    out.push_str(&format!(
        "pub const COMPILER_OSCILLATOR_R_BASE_OHM: u32 = {};\n",
        INKPLATE_OSCILLATOR.r_base_ohm
    ));
    out.push_str(&format!(
        "pub const COMPILER_OSCILLATOR_R_POT_OHM: u32 = {};\n",
        INKPLATE_OSCILLATOR.r_pot_ohm
    ));
    out.push_str(&format!(
        "pub const COMPILER_OSCILLATOR_CODE_MAX: u32 = {};\n",
        INKPLATE_OSCILLATOR.code_max
    ));
    out.push_str(&format!(
        "pub const COMPILER_TRANSPOSE: i32 = {transpose};\n"
    ));
    out.push('\n');
    out.push_str("/// One named notification score: borrowed elements only, no heap.\n");
    out.push_str("pub struct Notification {\n");
    out.push_str("    /// Catalogue name (the RTTTL score name).\n");
    out.push_str("    pub name: &'static str,\n");
    out.push_str("    /// Borrowed score; `p` rests between notes are part of the sound.\n");
    out.push_str("    pub score: &'static [Element],\n");
    out.push_str("}\n");
    for entry in entries {
        let total_ms = total_duration_ms(&entry.elements);
        out.push('\n');
        out.push_str(&format!(
            "/// `{}`: `{}` ({total_ms} ms).\n",
            entry.name, entry.rtttl
        ));
        out.push_str(&format!(
            "pub static {}: [Element; {}] = [\n",
            entry.ident,
            entry.elements.len()
        ));
        for element in &entry.elements {
            match *element {
                buzzer::score::Element::Note(note) => {
                    out.push_str("    Element::Note(Note {\n");
                    out.push_str(&format!("        code: {},\n", note.code));
                    out.push_str(&format!("        duration_ms: {},\n", note.duration_ms));
                    out.push_str("    }),\n");
                }
                buzzer::score::Element::Rest { duration_ms } => {
                    out.push_str(&format!(
                        "    Element::Rest {{ duration_ms: {duration_ms} }},\n"
                    ));
                }
            }
        }
        out.push_str("];\n");
    }
    out.push('\n');
    out.push_str("/// Whole catalogue in catalogue order.\n");
    if entries.len() == 1 {
        // rustfmt collapses a single-element struct array onto the outer
        // line; emit that canonical shape so generated output is already
        // `rustfmt --check` clean without touching multi-entry bytes.
        let entry = &entries[0];
        out.push_str("pub static CATALOGUE: [Notification; 1] = [Notification {\n");
        out.push_str(&format!("    name: {:?},\n", entry.name));
        out.push_str(&format!("    score: &{},\n", entry.ident));
        out.push_str("}];\n");
    } else {
        out.push_str(&format!(
            "pub static CATALOGUE: [Notification; {}] = [\n",
            entries.len()
        ));
        for entry in entries {
            out.push_str("    Notification {\n");
            out.push_str(&format!("        name: {:?},\n", entry.name));
            out.push_str(&format!("        score: &{},\n", entry.ident));
            out.push_str("    },\n");
        }
        out.push_str("];\n");
    }
    out
}
