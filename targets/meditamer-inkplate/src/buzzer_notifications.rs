//! Short notification scores for the Inkplate buzzer probe.
//!
//! Generated from `notifications.rtttl` (transpose +0) by the
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

/// `ack`: `ack:d=16,o=6,b=120:c,32p,e` (313 ms).
pub static ACK: [Element; 3] = [
    Element::Note(Note {
        code: 58,
        duration_ms: 125,
    }),
    Element::Rest { duration_ms: 63 },
    Element::Note(Note {
        code: 40,
        duration_ms: 125,
    }),
];

/// `attention`: `attention:d=16,o=6,b=120:e,32p,e` (313 ms).
pub static ATTENTION: [Element; 3] = [
    Element::Note(Note {
        code: 40,
        duration_ms: 125,
    }),
    Element::Rest { duration_ms: 63 },
    Element::Note(Note {
        code: 40,
        duration_ms: 125,
    }),
];

/// `complete`: `complete:d=16,o=6,b=120:c,32p,e,32p,g` (500 ms).
pub static COMPLETE: [Element; 5] = [
    Element::Note(Note {
        code: 58,
        duration_ms: 125,
    }),
    Element::Rest { duration_ms: 63 },
    Element::Note(Note {
        code: 40,
        duration_ms: 125,
    }),
    Element::Rest { duration_ms: 62 },
    Element::Note(Note {
        code: 29,
        duration_ms: 125,
    }),
];

/// Whole catalogue in catalogue order.
pub static CATALOGUE: [Notification; 3] = [
    Notification {
        name: "ack",
        score: &ACK,
    },
    Notification {
        name: "attention",
        score: &ATTENTION,
    },
    Notification {
        name: "complete",
        score: &COMPLETE,
    },
];
