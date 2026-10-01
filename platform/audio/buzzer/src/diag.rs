//! Bounded diagnostic playback operations.
//!
//! The diagnostic image exposes one bounded interface: raw code selection,
//! powered pitch changes, timed bursts, power cycles, a batch sweep, short
//! scores, and an explicit stop. Every operation validates its inputs and
//! durations *before* playback, expands into the same [`RequestedAction`]
//! stream the firmware runner executes, and reports compact trace lines with
//! requested and completed timestamps, selected codes, and errors.
//!
//! Durations and counts are deliberately small: diagnostics must not wedge
//! the shared I2C bus or abandon an enabled buzzer. [`Stop`] is always valid
//! and always expands to a single power-off.

use super::model::CODE_MAX;
use super::score::{plan, ActionKind, Element, Note, RequestedAction};
use core::fmt::Write;

/// Longest single hold/off/dwell in a diagnostic operation (milliseconds).
pub const MAX_DIAG_DURATION_MS: u32 = 5_000;

/// Most burst repeats in one operation.
pub const MAX_DIAG_REPEAT: u32 = 32;

/// Most score elements in one diagnostic score operation.
pub const MAX_DIAG_SCORE_ELEMENTS: usize = 32;

/// One validated diagnostic playback operation, times relative to its start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagOp<'a> {
    /// Power on, program a raw rheostat code, hold, then shut down.
    RawCode { code: u8, duration_ms: u32 },
    /// Change pitch while powered and hold. The runner equivalent keeps the
    /// rail on across the change; issuing this from rail-off powers on
    /// first, exactly like [`DiagOp::RawCode`].
    PitchHold { code: u8, duration_ms: u32 },
    /// `repeat` on/off cycles at one code. Every off interval power-cycles
    /// the rail, so bursts also sample the power-on default-pitch behavior.
    Burst {
        code: u8,
        on_ms: u32,
        off_ms: u32,
        repeat: u32,
    },
    /// Power off and stay silent. Measures the power-off tail; a following
    /// note must restore its code explicitly.
    PowerCycle { off_ms: u32 },
    /// Step every code from `from_code` to `to_code` inclusive, dwelling
    /// `dwell_ms` each. Powered once, then pitch-only changes.
    Sweep {
        from_code: u8,
        to_code: u8,
        dwell_ms: u32,
    },
    /// A bounded borrowed score; rests power-cycle as in [`plan`].
    Score(&'a [Element]),
    /// Explicit stop: power off immediately.
    Stop,
}

/// Why a diagnostic operation was rejected before playback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagError {
    /// A raw code exceeds [`CODE_MAX`].
    CodeOutOfRange(u8),
    /// A duration is zero or exceeds [`MAX_DIAG_DURATION_MS`].
    DurationOutOfRange(u32),
    /// A burst repeat is zero or exceeds [`MAX_DIAG_REPEAT`].
    RepeatOutOfRange(u32),
    /// A score is empty or exceeds [`MAX_DIAG_SCORE_ELEMENTS`] elements.
    ScoreOutOfRange(usize),
    /// A score element names a code above [`CODE_MAX`].
    ScoreCodeOutOfRange(u8),
    /// A score element has a zero or over-long duration.
    ScoreDurationOutOfRange(u32),
    /// A sweep names a code above [`CODE_MAX`].
    SweepCodeOutOfRange(u8),
    /// The caller's action buffer is too small; carries the required length.
    TraceTooSmall(usize),
}

fn check_code(code: u8) -> Result<(), DiagError> {
    if code > CODE_MAX {
        return Err(DiagError::CodeOutOfRange(code));
    }
    Ok(())
}

fn check_duration(duration_ms: u32) -> Result<(), DiagError> {
    if duration_ms == 0 || duration_ms > MAX_DIAG_DURATION_MS {
        return Err(DiagError::DurationOutOfRange(duration_ms));
    }
    Ok(())
}

/// Rejects invalid operations before playback; valid operations expand
/// without further checks.
pub fn validate(op: &DiagOp<'_>) -> Result<(), DiagError> {
    match *op {
        DiagOp::RawCode { code, duration_ms } | DiagOp::PitchHold { code, duration_ms } => {
            check_code(code)?;
            check_duration(duration_ms)?;
            Ok(())
        }
        DiagOp::Burst {
            code,
            on_ms,
            off_ms,
            repeat,
        } => {
            check_code(code)?;
            check_duration(on_ms)?;
            check_duration(off_ms)?;
            if repeat == 0 || repeat > MAX_DIAG_REPEAT {
                return Err(DiagError::RepeatOutOfRange(repeat));
            }
            Ok(())
        }
        DiagOp::PowerCycle { off_ms } => {
            check_duration(off_ms)?;
            Ok(())
        }
        DiagOp::Sweep {
            from_code,
            to_code,
            dwell_ms,
        } => {
            if from_code > CODE_MAX {
                return Err(DiagError::SweepCodeOutOfRange(from_code));
            }
            if to_code > CODE_MAX {
                return Err(DiagError::SweepCodeOutOfRange(to_code));
            }
            check_duration(dwell_ms)?;
            Ok(())
        }
        DiagOp::Score(elements) => {
            if elements.is_empty() || elements.len() > MAX_DIAG_SCORE_ELEMENTS {
                return Err(DiagError::ScoreOutOfRange(elements.len()));
            }
            for element in elements {
                match *element {
                    Element::Note(note) => {
                        if note.code > CODE_MAX {
                            return Err(DiagError::ScoreCodeOutOfRange(note.code));
                        }
                        if note.duration_ms == 0 || note.duration_ms > MAX_DIAG_DURATION_MS {
                            return Err(DiagError::ScoreDurationOutOfRange(note.duration_ms));
                        }
                    }
                    Element::Rest { duration_ms } => {
                        if duration_ms == 0 || duration_ms > MAX_DIAG_DURATION_MS {
                            return Err(DiagError::ScoreDurationOutOfRange(duration_ms));
                        }
                    }
                }
            }
            Ok(())
        }
        DiagOp::Stop => Ok(()),
    }
}

/// Short label used in compact trace lines.
pub fn op_label(op: &DiagOp<'_>) -> &'static str {
    match *op {
        DiagOp::RawCode { .. } => "raw",
        DiagOp::PitchHold { .. } => "pitch",
        DiagOp::Burst { .. } => "burst",
        DiagOp::PowerCycle { .. } => "power_cycle",
        DiagOp::Sweep { .. } => "sweep",
        DiagOp::Score(_) => "score",
        DiagOp::Stop => "stop",
    }
}

/// Expands a validated operation into timestamped actions (relative to
/// operation start) in `out`, returning the count written.
///
/// Returns [`DiagError::TraceTooSmall`] with the required length when `out`
/// is short; the bound is exact, so the caller can retry with a fitting
/// buffer. Unvalidated operations can still expand, but callers must
/// [`validate`] first: over-long inputs are the caller's bug, not a longer
/// playback.
pub fn expand_into(op: &DiagOp<'_>, out: &mut [RequestedAction]) -> Result<usize, DiagError> {
    let required = expanded_len(op);
    if out.len() < required {
        return Err(DiagError::TraceTooSmall(required));
    }
    let mut count = 0usize;
    let mut emit = |at_ms: u64, kind: ActionKind| {
        out[count] = RequestedAction { at_ms, kind };
        count += 1;
    };
    match *op {
        DiagOp::RawCode { code, .. } | DiagOp::PitchHold { code, .. } => {
            emit(0, ActionKind::PowerOnSetCode { code });
        }
        DiagOp::Burst {
            code,
            on_ms,
            off_ms,
            repeat,
        } => {
            let mut at_ms = 0u64;
            for _ in 0..repeat {
                emit(at_ms, ActionKind::PowerOnSetCode { code });
                at_ms += u64::from(on_ms);
                emit(at_ms, ActionKind::PowerOff);
                at_ms += u64::from(off_ms);
            }
        }
        DiagOp::PowerCycle { .. } => {
            emit(0, ActionKind::PowerOff);
        }
        DiagOp::Sweep {
            from_code,
            to_code,
            dwell_ms,
        } => {
            let mut at_ms = 0u64;
            let mut code = from_code;
            emit(0, ActionKind::PowerOnSetCode { code });
            at_ms += u64::from(dwell_ms);
            while code != to_code {
                if to_code > code {
                    code += 1;
                } else {
                    code -= 1;
                }
                emit(at_ms, ActionKind::SetCode { code });
                at_ms += u64::from(dwell_ms);
            }
        }
        DiagOp::Score(elements) => {
            for action in plan(elements) {
                emit(action.at_ms, action.kind);
            }
        }
        DiagOp::Stop => {
            emit(0, ActionKind::PowerOff);
        }
    }
    Ok(count)
}

/// Exact action count [`expand_into`] will write for `op`.
pub fn expanded_len(op: &DiagOp<'_>) -> usize {
    match *op {
        DiagOp::RawCode { .. } | DiagOp::PitchHold { .. } => 1,
        DiagOp::PowerCycle { .. } | DiagOp::Stop => 1,
        DiagOp::Burst { repeat, .. } => (repeat as usize) * 2,
        DiagOp::Sweep {
            from_code, to_code, ..
        } => from_code.abs_diff(to_code) as usize + 1,
        DiagOp::Score(elements) => plan(elements).count(),
    }
}

/// Writes one compact trace line for a finished operation:
///
/// ```text
/// BUZZ op=<label> t_req=<ms> t_done=<ms> code=<code|-> err=<none|label>
/// ```
///
/// `code` is the last selected code when the operation selects one.
pub fn write_trace_line<W: Write>(
    out: &mut W,
    op: &DiagOp<'_>,
    requested_ms: u64,
    completed_ms: u64,
    code: Option<u8>,
    err: Option<&str>,
) -> core::fmt::Result {
    out.write_str("BUZZ op=")?;
    out.write_str(op_label(op))?;
    write!(out, " t_req={requested_ms} t_done={completed_ms} code=")?;
    match code {
        Some(code) => write!(out, "{code}")?,
        None => out.write_str("-")?,
    }
    out.write_str(" err=")?;
    match err {
        Some(err) => out.write_str(err)?,
        None => out.write_str("none")?,
    }
    out.write_str("\r\n")
}

/// Builds the single-burst score [`DiagOp::RawCode`] plays: a note plus the
/// runner-owned terminal shutdown, for callers that need a [`Note`] view.
pub fn single_note(code: u8, duration_ms: u32) -> Note {
    Note { code, duration_ms }
}

#[cfg(test)]
mod tests;
