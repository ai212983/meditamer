//! Exact planner traces: command order, durations, rests and restarts.
extern crate std;

use super::*;

fn collect(elements: &[Element]) -> std::vec::Vec<RequestedAction> {
    plan(elements).collect()
}

#[test]
fn single_note_powers_on_with_its_code() {
    let score = [Element::Note(Note {
        code: 62,
        duration_ms: 100,
    })];
    assert_eq!(
        collect(&score),
        std::vec![RequestedAction {
            at_ms: 0,
            kind: ActionKind::PowerOnSetCode { code: 62 },
        }],
    );
}

#[test]
fn adjacent_notes_change_pitch_without_power_cycle() {
    let score = [
        Element::Note(Note {
            code: 10,
            duration_ms: 50,
        }),
        Element::Note(Note {
            code: 62,
            duration_ms: 70,
        }),
        Element::Note(Note {
            code: 127,
            duration_ms: 30,
        }),
    ];
    assert_eq!(
        collect(&score),
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
                kind: ActionKind::SetCode { code: 127 },
            },
        ],
    );
}

#[test]
fn rest_powers_off_and_next_note_restores_its_code() {
    let score = [
        Element::Note(Note {
            code: 62,
            duration_ms: 80,
        }),
        Element::Rest { duration_ms: 40 },
        Element::Note(Note {
            code: 62,
            duration_ms: 80,
        }),
    ];
    // Same code on both sides, yet the rest forces a power cycle: the rail
    // powers the rheostat, so power-up reloads the default code and the
    // requested code must be restored explicitly.
    assert_eq!(
        collect(&score),
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 62 },
            },
            RequestedAction {
                at_ms: 80,
                kind: ActionKind::PowerOff,
            },
            RequestedAction {
                at_ms: 120,
                kind: ActionKind::PowerOnSetCode { code: 62 },
            },
        ],
    );
}

#[test]
fn consecutive_rests_merge_into_one_power_off() {
    let score = [
        Element::Note(Note {
            code: 5,
            duration_ms: 10,
        }),
        Element::Rest { duration_ms: 10 },
        Element::Rest { duration_ms: 20 },
        Element::Note(Note {
            code: 9,
            duration_ms: 10,
        }),
    ];
    assert_eq!(
        collect(&score),
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 5 },
            },
            RequestedAction {
                at_ms: 10,
                kind: ActionKind::PowerOff,
            },
            RequestedAction {
                at_ms: 40,
                kind: ActionKind::PowerOnSetCode { code: 9 },
            },
        ],
    );
}

#[test]
fn leading_rest_and_zero_durations_emit_nothing() {
    let score = [
        Element::Rest { duration_ms: 25 },
        Element::Note(Note {
            code: 40,
            duration_ms: 0,
        }),
        Element::Rest { duration_ms: 0 },
        Element::Note(Note {
            code: 40,
            duration_ms: 15,
        }),
    ];
    assert_eq!(
        collect(&score),
        std::vec![RequestedAction {
            at_ms: 25,
            kind: ActionKind::PowerOnSetCode { code: 40 },
        }],
    );
}

#[test]
fn empty_score_emits_no_actions() {
    assert_eq!(collect(&[]), std::vec![]);
}
