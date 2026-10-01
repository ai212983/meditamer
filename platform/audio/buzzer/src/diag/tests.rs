//! Diagnostic validation, expansion, and trace-line tests.
extern crate std;

use super::*;
use crate::score::{ActionKind, Element, Note, RequestedAction};

fn expand(op: &DiagOp<'_>) -> std::vec::Vec<RequestedAction> {
    validate(op).expect("test operation must validate");
    let mut out = [RequestedAction {
        at_ms: 0,
        kind: ActionKind::PowerOff,
    }; 512];
    let count = expand_into(op, &mut out).expect("test buffer must fit");
    assert_eq!(count, expanded_len(op));
    out[..count].to_vec()
}

#[test]
fn validation_rejects_every_out_of_range_input() {
    assert_eq!(
        validate(&DiagOp::RawCode {
            code: 128,
            duration_ms: 10,
        }),
        Err(DiagError::CodeOutOfRange(128))
    );
    assert_eq!(
        validate(&DiagOp::RawCode {
            code: 5,
            duration_ms: 0,
        }),
        Err(DiagError::DurationOutOfRange(0))
    );
    assert_eq!(
        validate(&DiagOp::RawCode {
            code: 5,
            duration_ms: MAX_DIAG_DURATION_MS + 1,
        }),
        Err(DiagError::DurationOutOfRange(MAX_DIAG_DURATION_MS + 1))
    );
    assert_eq!(
        validate(&DiagOp::Burst {
            code: 5,
            on_ms: 10,
            off_ms: 10,
            repeat: 0,
        }),
        Err(DiagError::RepeatOutOfRange(0))
    );
    assert_eq!(
        validate(&DiagOp::Burst {
            code: 5,
            on_ms: 10,
            off_ms: 10,
            repeat: MAX_DIAG_REPEAT + 1,
        }),
        Err(DiagError::RepeatOutOfRange(MAX_DIAG_REPEAT + 1))
    );
    assert_eq!(
        validate(&DiagOp::Sweep {
            from_code: 200,
            to_code: 0,
            dwell_ms: 10,
        }),
        Err(DiagError::SweepCodeOutOfRange(200))
    );
    assert_eq!(
        validate(&DiagOp::Score(&[])),
        Err(DiagError::ScoreOutOfRange(0))
    );
    assert_eq!(
        validate(&DiagOp::Score(&[Element::Note(Note {
            code: 130,
            duration_ms: 10,
        })])),
        Err(DiagError::ScoreCodeOutOfRange(130))
    );
    assert_eq!(
        validate(&DiagOp::Score(&[Element::Rest { duration_ms: 0 }])),
        Err(DiagError::ScoreDurationOutOfRange(0))
    );
    assert!(validate(&DiagOp::Stop).is_ok());
    assert!(validate(&DiagOp::RawCode {
        code: CODE_MAX,
        duration_ms: MAX_DIAG_DURATION_MS,
    })
    .is_ok());
}

#[test]
fn burst_expands_to_power_cycled_pairs() {
    let actions = expand(&DiagOp::Burst {
        code: 62,
        on_ms: 20,
        off_ms: 10,
        repeat: 2,
    });
    assert_eq!(
        actions,
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 62 },
            },
            RequestedAction {
                at_ms: 20,
                kind: ActionKind::PowerOff,
            },
            RequestedAction {
                at_ms: 30,
                kind: ActionKind::PowerOnSetCode { code: 62 },
            },
            RequestedAction {
                at_ms: 50,
                kind: ActionKind::PowerOff,
            },
        ]
    );
}

#[test]
fn sweep_powers_once_then_changes_pitch() {
    let actions = expand(&DiagOp::Sweep {
        from_code: 0,
        to_code: 2,
        dwell_ms: 15,
    });
    assert_eq!(
        actions,
        std::vec![
            RequestedAction {
                at_ms: 0,
                kind: ActionKind::PowerOnSetCode { code: 0 },
            },
            RequestedAction {
                at_ms: 15,
                kind: ActionKind::SetCode { code: 1 },
            },
            RequestedAction {
                at_ms: 30,
                kind: ActionKind::SetCode { code: 2 },
            },
        ]
    );
    // Descending sweeps step down without reprising the power-on.
    let down = expand(&DiagOp::Sweep {
        from_code: 3,
        to_code: 1,
        dwell_ms: 5,
    });
    assert_eq!(down.len(), 3);
    assert!(matches!(
        down[0].kind,
        ActionKind::PowerOnSetCode { code: 3 }
    ));
    assert!(matches!(down[1].kind, ActionKind::SetCode { code: 2 }));
}

#[test]
fn score_op_matches_planner_output() {
    let elements = [
        Element::Note(Note {
            code: 7,
            duration_ms: 40,
        }),
        Element::Rest { duration_ms: 10 },
        Element::Note(Note {
            code: 9,
            duration_ms: 40,
        }),
    ];
    let actions = expand(&DiagOp::Score(&elements));
    assert_eq!(actions, plan(&elements).collect::<std::vec::Vec<_>>());
}

#[test]
fn expansion_reports_exact_overflow() {
    let op = DiagOp::Sweep {
        from_code: 0,
        to_code: 10,
        dwell_ms: 5,
    };
    assert_eq!(expanded_len(&op), 11);
    let mut short = [RequestedAction {
        at_ms: 0,
        kind: ActionKind::PowerOff,
    }; 4];
    assert_eq!(
        expand_into(&op, &mut short),
        Err(DiagError::TraceTooSmall(11))
    );
}

#[test]
fn trace_lines_record_codes_and_errors_compactly() {
    let op = DiagOp::RawCode {
        code: 62,
        duration_ms: 100,
    };
    let mut line = std::string::String::new();
    write_trace_line(&mut line, &op, 12, 118, Some(62), None).unwrap();
    assert_eq!(line, "BUZZ op=raw t_req=12 t_done=118 code=62 err=none\r\n");

    let stop = DiagOp::Stop;
    let mut failed = std::string::String::new();
    write_trace_line(&mut failed, &stop, 200, 201, None, Some("shutdown_fault")).unwrap();
    assert_eq!(
        failed,
        "BUZZ op=stop t_req=200 t_done=201 code=- err=shutdown_fault\r\n"
    );
}
