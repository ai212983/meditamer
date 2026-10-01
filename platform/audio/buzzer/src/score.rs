//! Borrowed scores and the event-deadline planner.
//!
//! A score is a borrowed slice of notes and rests. The planner walks it once
//! and emits one timestamped [`RequestedAction`] per note/rest boundary: no
//! fixed tick, only event deadlines. Adjacent notes emit [`ActionKind::SetCode`]
//! without interrupting power; a rest emits [`ActionKind::PowerOff`] so the
//! next note re-applies its code with [`ActionKind::PowerOnSetCode`] after
//! power-up. Consecutive rests merge into one power-off, and zero-duration
//! elements are skipped.
//!
//! The planner never emits the terminal shutdown: end, explicit stop, and
//! cancellation all run an awaited shutdown path owned by the runner, so both
//! runners observe the same command semantics.

use super::model::CODE_MAX;

/// One sounded pitch: a raw oscillator code held for `duration_ms`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Note {
    /// Raw oscillator code (`0..=CODE_MAX`); mapping Hz to codes is the
    /// board layer's job.
    pub code: u8,
    /// Audible hold time in milliseconds; zero is skipped by the planner.
    pub duration_ms: u32,
}

/// One score element: a sounded note or a silent rest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Element {
    Note(Note),
    Rest {
        /// Silence in milliseconds; zero is skipped by the planner.
        duration_ms: u32,
    },
}

/// One requested rail/pitch operation at an absolute score time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionKind {
    /// Power the rail on, wait the board startup interval, then program
    /// `code`. The rail powers the rheostat, so programming before the rail
    /// is settled addresses the power-on default code instead.
    PowerOnSetCode { code: u8 },
    /// Change pitch while the rail stays powered. Never emitted across a
    /// rest: every rest powers off, and every note after a rest restores its
    /// code with [`ActionKind::PowerOnSetCode`].
    SetCode { code: u8 },
    /// Power the rail off. Silence; the next note re-applies its code.
    PowerOff,
}

/// A rail/pitch operation requested at an absolute score time in
/// milliseconds from score start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestedAction {
    pub at_ms: u64,
    pub kind: ActionKind,
}

/// Full sounded-and-silent duration of `elements` in milliseconds.
///
/// The runner holds this duration before its terminal shutdown so playback
/// has one deterministic end for sleep coordination; the recording runner
/// timestamps its terminal shutdown with it.
pub fn total_duration_ms(elements: &[Element]) -> u64 {
    let mut total = 0u64;
    for element in elements {
        let duration_ms = match *element {
            Element::Note(note) => note.duration_ms,
            Element::Rest { duration_ms } => duration_ms,
        };
        total = total.saturating_add(u64::from(duration_ms));
    }
    total
}

/// Plans `elements` into timestamped actions.
///
/// The item stream borrows the score and carries the running time; both the
/// firmware runner and the host recording runner consume the same plan, and
/// the terminal shutdown is the runner's job, not the plan's.
pub fn plan(elements: &[Element]) -> Plan<'_> {
    Plan {
        elements,
        index: 0,
        at_ms: 0,
        powered: false,
    }
}

/// Lazy planner over a borrowed score; see [`plan`].
#[derive(Clone, Debug)]
pub struct Plan<'a> {
    elements: &'a [Element],
    index: usize,
    at_ms: u64,
    powered: bool,
}

impl Iterator for Plan<'_> {
    type Item = RequestedAction;

    fn next(&mut self) -> Option<RequestedAction> {
        while self.index < self.elements.len() {
            let duration_ms = match self.elements[self.index] {
                Element::Note(note) => note.duration_ms,
                Element::Rest { duration_ms } => duration_ms,
            };
            self.index += 1;
            if duration_ms == 0 {
                continue;
            }
            let at_ms = self.at_ms;
            self.at_ms += u64::from(duration_ms);
            match self.elements[self.index - 1] {
                Element::Note(note) => {
                    debug_assert!(note.code <= CODE_MAX);
                    if self.powered {
                        return Some(RequestedAction {
                            at_ms,
                            kind: ActionKind::SetCode { code: note.code },
                        });
                    }
                    self.powered = true;
                    return Some(RequestedAction {
                        at_ms,
                        kind: ActionKind::PowerOnSetCode { code: note.code },
                    });
                }
                Element::Rest { .. } => {
                    if self.powered {
                        self.powered = false;
                        return Some(RequestedAction {
                            at_ms,
                            kind: ActionKind::PowerOff,
                        });
                    }
                    continue;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests;
