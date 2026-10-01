//! Short notification scores for the Inkplate buzzer probe.
//!
//! Generated from `melodies.rtttl` (transpose +0) by the
//! `rtttl_catalogue` host tool; see
//! `docs/guides/audio/buzzer-listening.md` for the regenerate command. Do not
//! edit: change the catalogue and regenerate. These are listening
//! candidates, not acoustically selected presets.
use buzzer::score::{Element, Note};

/// Oscillator model the host compiler quantized through, as
/// read-only provenance for the firmware guard: the probe compares
/// these against the board `OSCILLATOR` and refuses on mismatch.
pub const COMPILER_OSCILLATOR_R_BASE_OHM: u32 = 2300;
pub const COMPILER_OSCILLATOR_R_POT_OHM: u32 = 10000;
pub const COMPILER_OSCILLATOR_CODE_MAX: u32 = 127;
pub const COMPILER_TRANSPOSE: i32 = 0;

/// One named notification score: borrowed elements only, no heap.
pub struct Notification {
    /// Catalogue name (the RTTTL score name).
    pub name: &'static str,
    /// Borrowed score; `p` rests between notes are part of the sound.
    pub score: &'static [Element],
}

/// `twinkle`: `twinkle:d=8,o=6,b=120:c.,16p,c.,16p,g.,16p,g.,16p,a.,16p,a.,16p,4g.,8p,f.,16p,f.,16p,e.,16p,e.,16p,d.,16p,d.,16p,4c.,8p` (8000 ms).
pub static TWINKLE: [Element; 28] = [
    Element::Note(Note {
        code: 58,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 58,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 29,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 29,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 23,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 23,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 29,
        duration_ms: 750,
    }),
    Element::Rest { duration_ms: 250 },
    Element::Note(Note {
        code: 36,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 36,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 40,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 40,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 49,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 49,
        duration_ms: 375,
    }),
    Element::Rest { duration_ms: 125 },
    Element::Note(Note {
        code: 58,
        duration_ms: 750,
    }),
    Element::Rest { duration_ms: 250 },
];

/// Whole catalogue in catalogue order.
pub static CATALOGUE: [Notification; 1] = [Notification {
    name: "twinkle",
    score: &TWINKLE,
}];
