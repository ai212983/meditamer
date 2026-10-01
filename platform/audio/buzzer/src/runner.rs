//! Asynchronous, fallible execution contract for buzzer scores.
//!
//! [`run`] drives one borrowed score through an [`Actuator`] against a
//! [`Clock`], with cooperative cancellation through [`Cancel`]:
//!
//! - Every action carries an absolute deadline. The runner waits until that
//!   deadline, then applies the action. An action whose interval already
//!   ended is skipped and counted, never burst through as catch-up; an
//!   action that is late but still live is applied reconciled against the
//!   actual rail state (a pitch change on an unpowered rail powers on with
//!   the current code, a redundant power-off on an already-off rail is
//!   skipped). Needed shutdowns are therefore applied even when late.
//! - Any [`Actuator`] failure halts playback immediately: the runner still
//!   runs the awaited shutdown path, then reports the original failure plus
//!   any shutdown failure. A failed shutdown is an explicit fault, never a
//!   silent success.
//! - End of score, explicit stop, and cancellation all run the same awaited
//!   shutdown. The runner additionally holds the score's full duration
//!   before the terminal shutdown, so sleep coordination observes one
//!   deterministic playback end.
//! - Cancellation is cooperative and takes effect at event boundaries only.
//!   Dropping the returned future is not cleanup: it abandons whatever rail
//!   state was last applied. Poll the future to completion, and do not sleep
//!   the board until it resolves.
//!
//! [`record`] is the host recording runner: it resolves the same plan plus
//! the same terminal shutdown into a caller-provided buffer, so CI compares
//! deterministic traces rather than rendered audio.

use super::score::{plan, total_duration_ms, ActionKind, Element, RequestedAction};

/// Fallible rail/pitch sink driven by the runner.
///
/// Firmware implements this over the board owner's rail enable, pitch
/// programming, and startup wait; the previewer and host tests use fakes.
/// There is exactly one owner of the rail per playback: sharing one
/// actuator across concurrent runners is a caller bug.
#[allow(async_fn_in_trait)]
pub trait Actuator {
    type Error;

    /// Apply one planned action at (or just after) its deadline.
    async fn apply(&mut self, action: RequestedAction) -> Result<(), Self::Error>;

    /// Awaited shutdown: power the rail off. Must be idempotent; the runner
    /// calls it even when the plan already ended powered off.
    async fn shutdown(&mut self) -> Result<(), Self::Error>;
}

/// Explicit millisecond clock, passed by the caller so host runs stay
/// deterministic without a timer driver.
#[allow(async_fn_in_trait)]
pub trait Clock {
    fn now_ms(&self) -> u64;

    /// Resolve no earlier than `deadline_ms`. Overshooting past the deadline
    /// is allowed; the runner re-checks and skips expired actions.
    async fn wait_until_ms(&mut self, deadline_ms: u64);
}

/// Cooperative cancellation flag checked at event boundaries.
pub trait Cancel {
    fn is_cancelled(&self) -> bool;
}

/// Counters describing one finished run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunStats {
    /// Planned actions applied (excludes the terminal shutdown).
    pub applied: u32,
    /// Planned actions skipped because their deadline had passed.
    pub skipped: u32,
    /// Whether the awaited shutdown path ran.
    pub shutdown_ran: bool,
}

/// How one run finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunOutcome<E> {
    /// The full score played and the terminal shutdown succeeded.
    Completed(RunStats),
    /// Cancellation was observed; the shutdown path still ran. A shutdown
    /// failure here converts to [`RunOutcome::Failed`]: an unpowered-down
    /// rail needs resolution before sleep, cancelled or not.
    Cancelled(RunStats),
    /// Playback halted. `error` is the failure that stopped the run (an
    /// action failure, a cancelled-or-terminal shutdown failure); a failed
    /// shutdown after an action failure is reported in `shutdown_error`
    /// rather than masking the original error.
    Failed {
        stats: RunStats,
        error: E,
        shutdown_error: Option<E>,
    },
}

async fn run_shutdown<A: Actuator>(actuator: &mut A, stats: &mut RunStats) -> Option<A::Error> {
    stats.shutdown_ran = true;
    actuator.shutdown().await.err()
}

/// Plays `elements` once; see the module docs for the contract.
pub async fn run<A: Actuator, C: Clock, X: Cancel>(
    elements: &[Element],
    actuator: &mut A,
    clock: &mut C,
    cancel: &X,
) -> RunOutcome<A::Error> {
    let mut stats = RunStats {
        applied: 0,
        skipped: 0,
        shutdown_ran: false,
    };
    let total_ms = total_duration_ms(elements);
    let mut plan_iter = plan(elements).peekable();
    // Actual rail state applied so far. A skipped initial power-on leaves
    // the rail off, so a later pitch change must power on with the current
    // code instead of running on an unpowered device.
    let mut powered = false;
    while let Some(action) = plan_iter.next() {
        if cancel.is_cancelled() {
            return finish_cancelled(actuator, stats).await;
        }
        if clock.now_ms() < action.at_ms {
            clock.wait_until_ms(action.at_ms).await;
            if cancel.is_cancelled() {
                return finish_cancelled(actuator, stats).await;
            }
        }
        let now_ms = clock.now_ms();
        // End of this action's interval: the next action's deadline, or the
        // score's full duration for the last action.
        let end_ms = plan_iter.peek().map_or(total_ms, |next| next.at_ms);
        let mut to_apply = action;
        if now_ms > action.at_ms {
            if now_ms >= end_ms {
                // Fully expired interval: skip without bursting obsolete
                // notes. A powered rail left by a skipped shutdown is
                // resolved by the next still-live interval or the terminal
                // shutdown below.
                stats.skipped += 1;
                continue;
            }
            // Late but still live: reconcile against the actual rail state.
            match action.kind {
                ActionKind::PowerOff if !powered => {
                    stats.skipped += 1;
                    continue;
                }
                ActionKind::SetCode { code } if !powered => {
                    to_apply = RequestedAction {
                        at_ms: action.at_ms,
                        kind: ActionKind::PowerOnSetCode { code },
                    };
                }
                _ => {}
            }
        } else {
            // On-time path keeps the same reconciliation so a redundant
            // power-off never touches the rail and a pitch change never
            // runs unpowered after an earlier skip.
            match action.kind {
                ActionKind::PowerOff if !powered => {
                    stats.skipped += 1;
                    continue;
                }
                ActionKind::SetCode { code } if !powered => {
                    to_apply = RequestedAction {
                        at_ms: action.at_ms,
                        kind: ActionKind::PowerOnSetCode { code },
                    };
                }
                _ => {}
            }
        }
        if let Err(error) = actuator.apply(to_apply).await {
            let shutdown_error = run_shutdown(actuator, &mut stats).await;
            return RunOutcome::Failed {
                stats,
                error,
                shutdown_error,
            };
        }
        stats.applied += 1;
        powered = !matches!(to_apply.kind, ActionKind::PowerOff);
    }
    if clock.now_ms() < total_ms {
        clock.wait_until_ms(total_ms).await;
        if cancel.is_cancelled() {
            return finish_cancelled(actuator, stats).await;
        }
    }
    if let Some(error) = run_shutdown(actuator, &mut stats).await {
        return RunOutcome::Failed {
            stats,
            error,
            shutdown_error: None,
        };
    }
    RunOutcome::Completed(stats)
}

async fn finish_cancelled<A: Actuator>(
    actuator: &mut A,
    mut stats: RunStats,
) -> RunOutcome<A::Error> {
    if let Some(error) = run_shutdown(actuator, &mut stats).await {
        return RunOutcome::Failed {
            stats,
            error,
            shutdown_error: None,
        };
    }
    RunOutcome::Cancelled(stats)
}

/// Deterministic trace produced by the host recording runner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordedTrace {
    /// Entries written, including the terminal shutdown.
    pub actions: usize,
    /// Score duration in milliseconds; the terminal shutdown's timestamp.
    pub total_ms: u64,
}

/// The caller's trace buffer was too small; nothing was partially reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordOverflow {
    pub capacity: usize,
    pub required: usize,
}

/// Resolves the same plan the firmware runner executes, plus the same
/// terminal shutdown at the score's full duration, into `out`.
///
/// On the happy path the firmware runner applies exactly this sequence, so a
/// trace mismatch between the two is a behavioral regression in one of them.
pub fn record(
    elements: &[Element],
    out: &mut [RequestedAction],
) -> Result<RecordedTrace, RecordOverflow> {
    let total_ms = total_duration_ms(elements);
    let mut count = 0usize;
    for action in plan(elements) {
        let Some(slot) = out.get_mut(count) else {
            return Err(RecordOverflow {
                capacity: out.len(),
                required: required_actions(elements),
            });
        };
        *slot = action;
        count += 1;
    }
    let Some(slot) = out.get_mut(count) else {
        return Err(RecordOverflow {
            capacity: out.len(),
            required: required_actions(elements),
        });
    };
    *slot = RequestedAction {
        at_ms: total_ms,
        kind: ActionKind::PowerOff,
    };
    count += 1;
    Ok(RecordedTrace {
        actions: count,
        total_ms,
    })
}

fn required_actions(elements: &[Element]) -> usize {
    plan(elements).count() + 1
}

#[cfg(test)]
mod tests;
