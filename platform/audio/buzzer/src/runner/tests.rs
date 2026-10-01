//! Runner contract: order, deadlines, failure, shutdown, trace parity.
extern crate std;

use super::*;
use crate::score::{ActionKind, Element, Note, RequestedAction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fault(&'static str);

struct FakeActuator {
    applied: std::vec::Vec<RequestedAction>,
    shutdowns: u32,
    fail_on_apply: Option<usize>,
    fail_shutdown: bool,
}

impl FakeActuator {
    fn new() -> Self {
        Self {
            applied: std::vec::Vec::new(),
            shutdowns: 0,
            fail_on_apply: None,
            fail_shutdown: false,
        }
    }
}

impl Actuator for FakeActuator {
    type Error = Fault;

    async fn apply(&mut self, action: RequestedAction) -> Result<(), Fault> {
        if self.fail_on_apply == Some(self.applied.len()) {
            return Err(Fault("apply"));
        }
        self.applied.push(action);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Fault> {
        self.shutdowns += 1;
        if self.fail_shutdown {
            return Err(Fault("shutdown"));
        }
        Ok(())
    }
}

struct FakeClock {
    now: u64,
    waits: std::vec::Vec<u64>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            now: 0,
            waits: std::vec::Vec::new(),
        }
    }

    fn starting_at(now: u64) -> Self {
        Self {
            now,
            waits: std::vec::Vec::new(),
        }
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.now
    }

    async fn wait_until_ms(&mut self, deadline_ms: u64) {
        self.waits.push(deadline_ms);
        self.now = deadline_ms;
    }
}

struct FakeCancel(bool);

impl Cancel for FakeCancel {
    fn is_cancelled(&self) -> bool {
        self.0
    }
}

fn block_on<F: core::future::Future>(future: F) -> F::Output {
    use core::task::{Context, Poll, Waker};
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

fn note(code: u8, duration_ms: u32) -> Element {
    Element::Note(Note { code, duration_ms })
}

#[test]
fn happy_path_applies_plan_then_terminal_shutdown() {
    let score = [
        note(10, 50),
        note(62, 70),
        Element::Rest { duration_ms: 30 },
    ];
    let mut actuator = FakeActuator::new();
    let mut clock = FakeClock::new();
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 3,
            skipped: 0,
            shutdown_ran: true,
        })
    );
    assert_eq!(
        actuator.applied,
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 10 },
            },
            RequestedAction {
                at_ms: 50,
                kind: ActionKind::SetCode { code: 62 },
            },
            RequestedAction {
                at_ms: 120,
                kind: ActionKind::PowerOff,
            },
        ]
    );
    assert_eq!(actuator.shutdowns, 1);
    // Absolute deadlines only: waits for 50, 120, then the full duration.
    assert_eq!(clock.waits, std::vec![50, 120, 150]);
}

/// A clock that overshoots one chosen deadline, modelling executor delay
/// rather than the runner bursting through a backlog.
struct JumpClock {
    inner: FakeClock,
    jump_deadline: u64,
    jump_to: u64,
}

impl Clock for JumpClock {
    fn now_ms(&self) -> u64 {
        self.inner.now
    }

    async fn wait_until_ms(&mut self, deadline_ms: u64) {
        self.inner.waits.push(deadline_ms);
        self.inner.now = if deadline_ms == self.jump_deadline {
            self.jump_to
        } else {
            deadline_ms
        };
    }
}

#[test]
fn expired_events_are_skipped_not_burst() {
    let score = [note(10, 50), note(62, 70)];
    let mut actuator = FakeActuator::new();
    // Blow the second deadline while waiting for it: the runner must skip
    // that pitch change instead of firing two actions back-to-back.
    let mut clock = JumpClock {
        inner: FakeClock::new(),
        jump_deadline: 50,
        jump_to: 500,
    };
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 1,
            skipped: 1,
            shutdown_ran: true,
        })
    );
    assert_eq!(actuator.applied.len(), 1);
    // No wait for the blown deadline's successor or the score end: by the
    // time the runner re-checks, both already passed.
    assert_eq!(clock.inner.waits, std::vec![50]);
}

#[test]
fn apply_failure_halts_and_reports_shutdown_failure() {
    let score = [note(10, 50), note(62, 70)];
    let mut actuator = FakeActuator::new();
    actuator.fail_on_apply = Some(1);
    actuator.fail_shutdown = true;
    let mut clock = FakeClock::new();
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            stats: RunStats {
                applied: 1,
                skipped: 0,
                shutdown_ran: true,
            },
            error: Fault("apply"),
            shutdown_error: Some(Fault("shutdown")),
        }
    );
    assert_eq!(actuator.shutdowns, 1);
}

#[test]
fn apply_failure_with_clean_shutdown_reports_original_only() {
    let score = [note(10, 50)];
    let mut actuator = FakeActuator::new();
    actuator.fail_on_apply = Some(0);
    let mut clock = FakeClock::new();
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            stats: RunStats {
                applied: 0,
                skipped: 0,
                shutdown_ran: true,
            },
            error: Fault("apply"),
            shutdown_error: None,
        }
    );
}

#[test]
fn terminal_shutdown_failure_is_a_fault_not_success() {
    let score = [note(10, 50)];
    let mut actuator = FakeActuator::new();
    actuator.fail_shutdown = true;
    let mut clock = FakeClock::new();
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            stats: RunStats {
                applied: 1,
                skipped: 0,
                shutdown_ran: true,
            },
            error: Fault("shutdown"),
            shutdown_error: None,
        }
    );
}

#[test]
fn cancellation_runs_shutdown_and_reports_cancelled() {
    let score = [note(10, 50), note(62, 70)];
    let mut actuator = FakeActuator::new();
    let mut clock = FakeClock::new();
    // Cancel before the first deadline: nothing applied, shutdown still ran.
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(true)));
    assert_eq!(
        outcome,
        RunOutcome::Cancelled(RunStats {
            applied: 0,
            skipped: 0,
            shutdown_ran: true,
        })
    );
    assert!(actuator.applied.is_empty());
    assert_eq!(actuator.shutdowns, 1);
}

#[test]
fn recording_matches_firmware_happy_path_trace() {
    let score = [
        note(10, 50),
        note(62, 70),
        Element::Rest { duration_ms: 30 },
        note(9, 20),
    ];
    let mut recorded = [RequestedAction {
        at_ms: 0,
        kind: ActionKind::PowerOff,
    }; 8];
    let trace = record(&score, &mut recorded).unwrap();
    assert_eq!(trace.total_ms, 170);
    assert_eq!(trace.actions, 5);
    assert_eq!(
        recorded[trace.actions - 1],
        RequestedAction {
            at_ms: 170,
            kind: ActionKind::PowerOff,
        }
    );

    let mut actuator = FakeActuator::new();
    let mut clock = FakeClock::new();
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert!(matches!(outcome, RunOutcome::Completed(_)));
    // Firmware applies the plan; the runner's terminal shutdown is the
    // recording's terminal entry. Same command semantics, bit for bit.
    actuator.applied.push(RequestedAction {
        at_ms: trace.total_ms,
        kind: ActionKind::PowerOff,
    });
    assert_eq!(&recorded[..trace.actions], actuator.applied.as_slice());
}

#[test]
fn late_power_off_still_sounds_rest_silence() {
    // Note into rest: a 1 ms scheduling delay past the rest boundary must
    // still power off rather than sounding throughout the rest.
    let score = [note(10, 50), Element::Rest { duration_ms: 30 }];
    let mut actuator = FakeActuator::new();
    let mut clock = JumpClock {
        inner: FakeClock::new(),
        jump_deadline: 50,
        jump_to: 51,
    };
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 2,
            skipped: 0,
            shutdown_ran: true,
        })
    );
    assert_eq!(
        actuator.applied,
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 10 },
            },
            RequestedAction {
                at_ms: 50,
                kind: ActionKind::PowerOff,
            },
        ]
    );
    // After the late shutdown the runner still holds the full duration.
    assert_eq!(clock.inner.waits, std::vec![50, 80]);
}

#[test]
fn delayed_initial_clock_retunes_current_interval() {
    // Clock already past the first note when playback starts: the expired
    // power-on is skipped, and the still-live pitch change powers on with
    // the current code instead of running SetCode on an unpowered rail.
    let score = [note(10, 10), note(62, 10)];
    let mut actuator = FakeActuator::new();
    let mut clock = FakeClock::starting_at(12);
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 1,
            skipped: 1,
            shutdown_ran: true,
        })
    );
    assert_eq!(
        actuator.applied,
        std::vec![RequestedAction {
            at_ms: 10,
            kind: ActionKind::PowerOnSetCode { code: 62 },
        }]
    );
}

#[test]
fn multiple_expired_intervals_apply_only_current() {
    // Four 10 ms notes with the clock starting 25 ms in: the first two
    // intervals are fully expired and skipped, the third is still live and
    // powers on with its code, and the fourth waits on its absolute
    // deadline.
    let score = [note(1, 10), note(2, 10), note(3, 10), note(4, 10)];
    let mut actuator = FakeActuator::new();
    let mut clock = FakeClock::starting_at(25);
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 2,
            skipped: 2,
            shutdown_ran: true,
        })
    );
    assert_eq!(
        actuator.applied,
        std::vec![
            RequestedAction {
                at_ms: 20,
                kind: ActionKind::PowerOnSetCode { code: 3 },
            },
            RequestedAction {
                at_ms: 30,
                kind: ActionKind::SetCode { code: 4 },
            },
        ]
    );
    assert_eq!(clock.waits, std::vec![30, 40]);
}

#[test]
fn action_duration_lateness_skips_fully_expired_tail() {
    // One applied action followed by an overshoot past the score end:
    // every remaining interval is fully expired, so all are skipped and
    // only the terminal shutdown follows.
    let score = [note(1, 10), note(2, 10), note(3, 10), note(4, 10)];
    let mut actuator = FakeActuator::new();
    let mut clock = JumpClock {
        inner: FakeClock::new(),
        jump_deadline: 10,
        jump_to: 45,
    };
    let outcome = block_on(run(&score, &mut actuator, &mut clock, &FakeCancel(false)));
    assert_eq!(
        outcome,
        RunOutcome::Completed(RunStats {
            applied: 1,
            skipped: 3,
            shutdown_ran: true,
        })
    );
    assert_eq!(
        actuator.applied,
        std::vec![RequestedAction {
            at_ms: 0,
            kind: ActionKind::PowerOnSetCode { code: 1 },
        }]
    );
    assert_eq!(clock.inner.waits, std::vec![10]);
}

#[test]
fn recording_reports_overflow_without_partial_claims() {
    let score = [note(10, 50), note(62, 70)];
    let mut tiny = [RequestedAction {
        at_ms: 0,
        kind: ActionKind::PowerOff,
    }; 1];
    assert_eq!(
        record(&score, &mut tiny),
        Err(RecordOverflow {
            capacity: 1,
            required: 3,
        })
    );
    let mut empty: [RequestedAction; 0] = [];
    assert_eq!(
        record(&[], &mut empty),
        Err(RecordOverflow {
            capacity: 0,
            required: 1,
        })
    );
}
