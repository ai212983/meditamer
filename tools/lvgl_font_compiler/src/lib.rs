//! Compiles a TrueType face into the `lv_font_fmt_txt` tables LVGL's built-in
//! fonts use, emitted as Rust source for a build script to write into
//! `OUT_DIR` and a firmware module to `include!`.
//!
//! It exists because LVGL ships Montserrat only up to 48 px, and the Ambient
//! Home clock needs 128 px (`docs/plans/ambient-home-prototype.md`). Only the
//! requested characters are rasterized, so a clock face costs a few kilobytes
//! of flash rather than a full Latin range.
//!
//! Glyph bitmaps are 1 bit per pixel. The panel is monochrome and the flush
//! path thresholds luminance at 128 (`src/firmware/ui/lvgl/dither.rs`), so for
//! black text on white every coverage value above 127 reaches the panel as
//! black and every other value as white: an anti-aliased ramp would quadruple
//! the table (4 bpp) without surviving the threshold.

use std::{fmt, fs, path::Path};

use fontdue::{Font, FontSettings};

/// LVGL reserves glyph id 0 for "not found", so the emitted `glyph_dsc` table
/// opens with an empty entry and the first real glyph is id 1.
const RESERVED_GLYPH_IDS: u16 = 1;

/// `lv_font_fmt_txt_glyph_dsc_t` packs `bitmap_index` into 20 bits and `adv_w`
/// into the remaining 12 of one 32-bit unit.
const MAX_BITMAP_INDEX: usize = (1 << 20) - 1;
const MAX_ADV_W: u32 = (1 << 12) - 1;

/// Coverage at or above this value becomes a set bit. Matches the flush
/// path's `dithered_black` threshold so a compiled glyph reaches the panel
/// exactly as the rasterizer drew it.
const COVERAGE_THRESHOLD: u8 = 128;

/// Bytes of glyph bitmap per emitted source line.
const BITMAP_BYTES_PER_LINE: usize = 16;

/// One font to compile.
pub struct FontSpec<'a> {
    /// TrueType face to rasterize. A variable face is rendered at its default
    /// instance.
    pub source: &'a Path,
    /// Rendering size in pixels per em.
    pub size_px: f32,
    /// Characters to include. Order and duplicates do not matter.
    pub characters: &'a str,
}

#[derive(Debug)]
pub enum FontCompilerError {
    Read(String, std::io::Error),
    Parse(String),
    NoCharacters,
    MissingGlyph(char),
    MissingLineMetrics,
    /// A compiled value does not fit the field `lv_font_fmt_txt` gives it.
    OutOfRange {
        character: char,
        field: &'static str,
        value: i64,
    },
}

impl fmt::Display for FontCompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, error) => write!(f, "failed to read {path}: {error}"),
            Self::Parse(message) => write!(f, "failed to parse the face: {message}"),
            Self::NoCharacters => write!(f, "no characters requested"),
            Self::MissingGlyph(character) => {
                write!(f, "the face has no glyph for {character:?}")
            }
            Self::MissingLineMetrics => write!(f, "the face has no horizontal line metrics"),
            Self::OutOfRange {
                character,
                field,
                value,
            } => write!(
                f,
                "{character:?} needs {field} = {value}, which does not fit the LVGL field; \
                 compile the face at a smaller size"
            ),
        }
    }
}

impl std::error::Error for FontCompilerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(_, error) => Some(error),
            _ => None,
        }
    }
}

/// One glyph as `lv_font_fmt_txt_glyph_dsc_t` describes it.
struct Glyph {
    character: char,
    /// Byte offset into the emitted bitmap table.
    bitmap_index: u32,
    /// Advance width in 1/16 px, the LVGL 12.4 fixed-point convention.
    adv_w: u16,
    box_w: u8,
    box_h: u8,
    ofs_x: i8,
    ofs_y: i8,
}

/// A run of consecutive code points, emitted as one `LV_FONT_FMT_TXT_CMAP_FORMAT0_TINY`
/// character map.
struct CodepointRun {
    start: u32,
    length: u16,
    first_glyph_id: u16,
}

/// Everything the rasterizer produced, ready to render as source.
struct CompiledFont {
    characters: Vec<char>,
    bitmap: Vec<u8>,
    glyphs: Vec<Glyph>,
    runs: Vec<CodepointRun>,
    line_height: i32,
    base_line: i32,
    /// Underline offset below the base line; also its thickness.
    underline: i32,
}

/// Compiles the Ambient Home clock face from `assets_dir`
/// (`docs/plans/ambient-home-prototype.md`): 128 px, and only the characters
/// `HH:MM` and its `--:--` placeholder need.
///
/// The firmware build and the UI host harness both call this so the table the
/// harness renders in its tests is the table the firmware ships.
pub fn generate_ambient_clock_font(assets_dir: &Path) -> Result<String, FontCompilerError> {
    generate_from_path(&FontSpec {
        source: &assets_dir.join("IBMPlexSans-Variable.ttf"),
        size_px: 128.0,
        characters: "0123456789:-",
    })
}

pub fn generate_from_path(spec: &FontSpec<'_>) -> Result<String, FontCompilerError> {
    let data = fs::read(spec.source)
        .map_err(|error| FontCompilerError::Read(spec.source.display().to_string(), error))?;
    let settings = FontSettings {
        scale: spec.size_px,
        ..FontSettings::default()
    };
    let face = Font::from_bytes(data.as_slice(), settings)
        .map_err(|error| FontCompilerError::Parse(error.to_string()))?;

    let mut characters: Vec<char> = spec.characters.chars().collect();
    characters.sort_unstable();
    characters.dedup();
    if characters.is_empty() {
        return Err(FontCompilerError::NoCharacters);
    }

    let mut bitmap: Vec<u8> = Vec::new();
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(characters.len());
    for &character in &characters {
        glyphs.push(compile_glyph(&face, spec.size_px, character, &mut bitmap)?);
    }

    let metrics = face
        .horizontal_line_metrics(spec.size_px)
        .ok_or(FontCompilerError::MissingLineMetrics)?;
    let line_height = (metrics.ascent - metrics.descent).round() as i32;
    let base_line = (-metrics.descent).round() as i32;
    // Mirrors the built-in Montserrat faces, whose 32 px table underlines at
    // -2 px with a 2 px stroke.
    let underline = (spec.size_px / 16.0).round().max(1.0) as i32;

    let runs = runs(&glyphs);
    Ok(render(
        spec,
        &CompiledFont {
            characters,
            bitmap,
            glyphs,
            runs,
            line_height,
            base_line,
            underline,
        },
    ))
}

fn compile_glyph(
    face: &Font,
    size_px: f32,
    character: char,
    bitmap: &mut Vec<u8>,
) -> Result<Glyph, FontCompilerError> {
    if !face.has_glyph(character) {
        return Err(FontCompilerError::MissingGlyph(character));
    }
    let (metrics, coverage) = face.rasterize(character, size_px);

    let bitmap_index = bitmap.len();
    if bitmap_index > MAX_BITMAP_INDEX {
        return Err(FontCompilerError::OutOfRange {
            character,
            field: "bitmap_index",
            value: bitmap_index as i64,
        });
    }
    append_packed(bitmap, &coverage);

    let adv_w = (metrics.advance_width * 16.0).round() as i64;
    Ok(Glyph {
        character,
        bitmap_index: bitmap_index as u32,
        adv_w: fits(character, "adv_w", adv_w, 0, i64::from(MAX_ADV_W))? as u16,
        box_w: fits(
            character,
            "box_w",
            metrics.width as i64,
            0,
            i64::from(u8::MAX),
        )? as u8,
        box_h: fits(
            character,
            "box_h",
            metrics.height as i64,
            0,
            i64::from(u8::MAX),
        )? as u8,
        ofs_x: fits(
            character,
            "ofs_x",
            i64::from(metrics.xmin),
            i64::from(i8::MIN),
            i64::from(i8::MAX),
        )? as i8,
        ofs_y: fits(
            character,
            "ofs_y",
            i64::from(metrics.ymin),
            i64::from(i8::MIN),
            i64::from(i8::MAX),
        )? as i8,
    })
}

fn fits(
    character: char,
    field: &'static str,
    value: i64,
    min: i64,
    max: i64,
) -> Result<i64, FontCompilerError> {
    if value < min || value > max {
        return Err(FontCompilerError::OutOfRange {
            character,
            field,
            value,
        });
    }
    Ok(value)
}

/// Packs one glyph's coverage into 1-bpp rows, most significant bit first.
///
/// The emitted descriptor leaves `stride` at 0, which is LVGL's "no padding"
/// setting: rows run on without realigning, and only the glyph as a whole
/// starts on a byte boundary.
fn append_packed(bitmap: &mut Vec<u8>, coverage: &[u8]) {
    let mut byte = 0u8;
    let mut filled = 0u32;
    for &value in coverage {
        byte <<= 1;
        if value >= COVERAGE_THRESHOLD {
            byte |= 1;
        }
        filled += 1;
        if filled == 8 {
            bitmap.push(byte);
            byte = 0;
            filled = 0;
        }
    }
    if filled > 0 {
        bitmap.push(byte << (8 - filled));
    }
}

/// Groups the glyphs, which are sorted by code point, into consecutive runs.
fn runs(glyphs: &[Glyph]) -> Vec<CodepointRun> {
    let mut runs: Vec<CodepointRun> = Vec::new();
    for (index, glyph) in glyphs.iter().enumerate() {
        let codepoint = u32::from(glyph.character);
        let glyph_id = index as u16 + RESERVED_GLYPH_IDS;
        match runs.last_mut() {
            Some(run) if run.start + u32::from(run.length) == codepoint => run.length += 1,
            _ => runs.push(CodepointRun {
                start: codepoint,
                length: 1,
                first_glyph_id: glyph_id,
            }),
        }
    }
    runs
}

fn render(spec: &FontSpec<'_>, font: &CompiledFont) -> String {
    format!(
        "// @generated by tools/lvgl_font_compiler from {source} at {size} px, 1 bpp.\n\
         // Characters: {characters:?}. Do not edit; change the build script instead.\n\
         \n\
         /// The LVGL tables hold raw pointers into each other, which keeps them\n\
         /// out of `static` without this wrapper. They are read-only for LVGL's\n\
         /// lifetime, so sharing them across threads is sound.\n\
         #[repr(transparent)]\n\
         struct FontTable<T>(T);\n\
         \n\
         unsafe impl<T> Sync for FontTable<T> {{}}\n\
         \n\
         const fn glyph_dsc(\n\
         \x20   bitmap_index: u32,\n\
         \x20   adv_w: u16,\n\
         \x20   box_w: u8,\n\
         \x20   box_h: u8,\n\
         \x20   ofs_x: i8,\n\
         \x20   ofs_y: i8,\n\
         ) -> ::lightvgl_sys::lv_font_fmt_txt_glyph_dsc_t {{\n\
         \x20   // `bitmap_index` occupies bits 0..20 and `adv_w` bits 20..32.\n\
         \x20   let packed = bitmap_index | ((adv_w as u32) << 20);\n\
         \x20   ::lightvgl_sys::lv_font_fmt_txt_glyph_dsc_t {{\n\
         \x20       _bitfield_align_1: [],\n\
         \x20       _bitfield_1: ::lightvgl_sys::__BindgenBitfieldUnit::new(packed.to_le_bytes()),\n\
         \x20       box_w,\n\
         \x20       box_h,\n\
         \x20       ofs_x,\n\
         \x20       ofs_y,\n\
         \x20   }}\n\
         }}\n\
         \n\
         static GLYPH_BITMAP: [u8; {bitmap_len}] = [\n\
         {bitmap}\
         ];\n\
         \n\
         static GLYPH_DSC: FontTable<[::lightvgl_sys::lv_font_fmt_txt_glyph_dsc_t; {glyph_count}]> =\n\
         \x20   FontTable([\n\
         \x20       // Glyph id 0 is LVGL's \"not found\" entry.\n\
         \x20       glyph_dsc(0, 0, 0, 0, 0, 0),\n\
         {glyph_dscs}\
         \x20   ]);\n\
         \n\
         static CMAPS: FontTable<[::lightvgl_sys::lv_font_fmt_txt_cmap_t; {cmap_count}]> =\n\
         \x20   FontTable([\n\
         {cmaps}\
         \x20   ]);\n\
         \n\
         static FONT_DSC: FontTable<::lightvgl_sys::lv_font_fmt_txt_dsc_t> =\n\
         \x20   FontTable(::lightvgl_sys::lv_font_fmt_txt_dsc_t {{\n\
         \x20       glyph_bitmap: GLYPH_BITMAP.as_ptr(),\n\
         \x20       glyph_dsc: GLYPH_DSC.0.as_ptr(),\n\
         \x20       cmaps: CMAPS.0.as_ptr(),\n\
         \x20       kern_dsc: ::core::ptr::null(),\n\
         \x20       kern_scale: 0,\n\
         \x20       _bitfield_align_1: [],\n\
         \x20       // cmap_num bits 0..9, bpp bits 9..13, kern_classes bit 13,\n\
         \x20       // bitmap_format bits 14..16 (0 = plain, uncompressed).\n\
         \x20       _bitfield_1: ::lightvgl_sys::__BindgenBitfieldUnit::new(\n\
         \x20           ({cmap_count}u16 | (1u16 << 9)).to_le_bytes(),\n\
         \x20       ),\n\
         \x20       // 0 means rows are packed without padding.\n\
         \x20       stride: 0,\n\
         \x20   }});\n\
         \n\
         static FONT: FontTable<::lightvgl_sys::lv_font_t> =\n\
         \x20   FontTable(::lightvgl_sys::lv_font_t {{\n\
         \x20       get_glyph_dsc: Some(::lightvgl_sys::lv_font_get_glyph_dsc_fmt_txt),\n\
         \x20       get_glyph_bitmap: Some(::lightvgl_sys::lv_font_get_bitmap_fmt_txt),\n\
         \x20       release_glyph: None,\n\
         \x20       line_height: {line_height},\n\
         \x20       base_line: {base_line},\n\
         \x20       _bitfield_align_1: [],\n\
         \x20       // subpx bits 0..2 (none), kerning bits 2..3 (normal),\n\
         \x20       // static_bitmap bit 3.\n\
         \x20       _bitfield_1: ::lightvgl_sys::__BindgenBitfieldUnit::new([0]),\n\
         \x20       underline_position: {underline_position},\n\
         \x20       underline_thickness: {underline_thickness},\n\
         \x20       dsc: &FONT_DSC.0 as *const ::lightvgl_sys::lv_font_fmt_txt_dsc_t\n\
         \x20           as *const ::core::ffi::c_void,\n\
         \x20       fallback: ::core::ptr::null(),\n\
         \x20       user_data: ::core::ptr::null_mut(),\n\
         \x20   }});\n\
         \n\
         /// The compiled face, for `lv_obj_set_style_text_font`.\n\
         pub fn font() -> *const ::lightvgl_sys::lv_font_t {{\n\
         \x20   &FONT.0\n\
         }}\n",
        source = source_name(spec.source),
        size = spec.size_px,
        characters = font.characters.iter().collect::<String>(),
        bitmap_len = font.bitmap.len(),
        bitmap = render_bitmap(&font.bitmap),
        glyph_count = font.glyphs.len() + usize::from(RESERVED_GLYPH_IDS),
        glyph_dscs = render_glyph_dscs(&font.glyphs),
        cmap_count = font.runs.len(),
        cmaps = render_cmaps(&font.runs),
        line_height = font.line_height,
        base_line = font.base_line,
        underline_position = -font.underline,
        underline_thickness = font.underline,
    )
}

/// The face's file name. The full path is deliberately left out: it is an
/// absolute build-machine path, and the repository keeps those out of
/// generated artifacts.
fn source_name(source: &Path) -> String {
    source
        .file_name()
        .unwrap_or(source.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn render_bitmap(bitmap: &[u8]) -> String {
    let mut rendered = String::new();
    for line in bitmap.chunks(BITMAP_BYTES_PER_LINE) {
        rendered.push_str("    ");
        for byte in line {
            rendered.push_str(&format!("{byte:#04x}, "));
        }
        rendered.pop();
        rendered.push('\n');
    }
    rendered
}

fn render_glyph_dscs(glyphs: &[Glyph]) -> String {
    let mut rendered = String::new();
    for glyph in glyphs {
        rendered.push_str(&format!(
            "        // {character:?}\n\
             \x20       glyph_dsc({bitmap_index}, {adv_w}, {box_w}, {box_h}, {ofs_x}, {ofs_y}),\n",
            character = glyph.character,
            bitmap_index = glyph.bitmap_index,
            adv_w = glyph.adv_w,
            box_w = glyph.box_w,
            box_h = glyph.box_h,
            ofs_x = glyph.ofs_x,
            ofs_y = glyph.ofs_y,
        ));
    }
    rendered
}

fn render_cmaps(runs: &[CodepointRun]) -> String {
    let mut rendered = String::new();
    for run in runs {
        rendered.push_str(&format!(
            "        ::lightvgl_sys::lv_font_fmt_txt_cmap_t {{\n\
             \x20           range_start: {start},\n\
             \x20           range_length: {length},\n\
             \x20           glyph_id_start: {first_glyph_id},\n\
             \x20           unicode_list: ::core::ptr::null(),\n\
             \x20           glyph_id_ofs_list: ::core::ptr::null(),\n\
             \x20           list_length: 0,\n\
             \x20           type_: ::lightvgl_sys::lv_font_fmt_txt_cmap_type_t_LV_FONT_FMT_TXT_CMAP_FORMAT0_TINY,\n\
             \x20       }},\n",
            start = run.start,
            length = run.length,
            first_glyph_id = run.first_glyph_id,
        ));
    }
    rendered
}
