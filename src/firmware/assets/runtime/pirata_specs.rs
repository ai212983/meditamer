#[derive(Clone, Copy)]
pub(super) struct PirataGlyphSpec {
    pub(super) glyph: char,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) bytes_len: usize,
    pub(super) path: &'static str,
}

pub(super) const PIRATA_GLYPH_SPECS: [PirataGlyphSpec; 11] = [
    PirataGlyphSpec {
        glyph: '0',
        width: 84,
        height: 192,
        bytes_len: 2112,
        path: "/assets/raw/fonts/pirata_clock/digit_0_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '1',
        width: 60,
        height: 193,
        bytes_len: 1544,
        path: "/assets/raw/fonts/pirata_clock/digit_1_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '2',
        width: 98,
        height: 192,
        bytes_len: 2496,
        path: "/assets/raw/fonts/pirata_clock/digit_2_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '3',
        width: 91,
        height: 194,
        bytes_len: 2328,
        path: "/assets/raw/fonts/pirata_clock/digit_3_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '4',
        width: 99,
        height: 192,
        bytes_len: 2496,
        path: "/assets/raw/fonts/pirata_clock/digit_4_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '5',
        width: 87,
        height: 200,
        bytes_len: 2200,
        path: "/assets/raw/fonts/pirata_clock/digit_5_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '6',
        width: 84,
        height: 192,
        bytes_len: 2112,
        path: "/assets/raw/fonts/pirata_clock/digit_6_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '7',
        width: 90,
        height: 198,
        bytes_len: 2376,
        path: "/assets/raw/fonts/pirata_clock/digit_7_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '8',
        width: 84,
        height: 192,
        bytes_len: 2112,
        path: "/assets/raw/fonts/pirata_clock/digit_8_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: '9',
        width: 84,
        height: 192,
        bytes_len: 2112,
        path: "/assets/raw/fonts/pirata_clock/digit_9_mono1.raw",
    },
    PirataGlyphSpec {
        glyph: ':',
        width: 32,
        height: 109,
        bytes_len: 436,
        path: "/assets/raw/fonts/pirata_clock/colon_mono1.raw",
    },
];
