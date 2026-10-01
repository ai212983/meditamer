#![allow(dead_code)]

#[path = "../../../products/meditamer/src/firmware/touch/admission/model.rs"]
mod model;
use model::Admission;

fn released() -> Admission {
    let mut gate = Admission::new();
    assert!(gate.observe(gate.epoch(), 0));
    gate
}

#[test]
fn queued_and_lookahead_input_never_reaches_destination() {
    let mut gate = released();
    let queued_epoch = gate.epoch();
    assert!(gate.observe(queued_epoch, 1));
    let transition = gate.begin_transition();
    assert!(!gate.accepts(queued_epoch));
    assert!(!gate.observe(gate.epoch(), 0));
    gate.presentation_complete(transition);
    assert!(!gate.accepts(queued_epoch));
    // The same rejection covers a timer-generated Up from the retired HSM.
    assert!(gate.observe(gate.epoch(), 1));
    assert!(gate.accepts(gate.epoch()));
}

#[test]
fn complete_transition_tap_is_discarded_but_next_tap_is_admitted() {
    let mut gate = released();
    let transition = gate.begin_transition();
    let during = gate.epoch();
    assert!(!gate.observe(during, 1));
    assert!(!gate.observe(during, 0));
    gate.presentation_complete(transition);
    assert!(!gate.accepts(during));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn held_contact_waits_for_release_after_presentation() {
    let mut gate = released();
    let transition = gate.begin_transition();
    assert!(!gate.observe(gate.epoch(), 1));
    gate.presentation_complete(transition);
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.accepts(gate.epoch()));
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn release_then_second_contact_during_transition_still_waits() {
    let mut gate = released();
    let transition = gate.begin_transition();
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(!gate.observe(gate.epoch(), 1));
    gate.presentation_complete(transition);
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn idle_transition_does_not_swallow_first_fresh_tap() {
    let mut gate = released();
    let transition = gate.begin_transition();
    gate.presentation_complete(transition);
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn old_read_cannot_clear_a_newer_held_contact() {
    let mut gate = released();
    let old_read = gate.epoch();
    let transition = gate.begin_transition();
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(old_read, 0));
    gate.presentation_complete(transition);
    assert!(!gate.observe(gate.epoch(), 1));
}

#[test]
fn read_started_during_transition_cannot_introduce_a_new_press() {
    let mut gate = released();
    let transition = gate.begin_transition();
    let read_epoch = gate.epoch();
    gate.presentation_complete(transition);
    assert!(!gate.observe(read_epoch, 1));
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn failed_or_stale_presentation_does_not_reopen_admission() {
    let mut gate = released();
    let first = gate.begin_transition();
    let second = gate.begin_transition();
    gate.presentation_complete(first);
    assert!(!gate.accepts(gate.epoch()));
    assert!(!gate.observe(gate.epoch(), 0));
    gate.presentation_complete(second);
    assert!(gate.observe(gate.epoch(), 1));
    let admitted = gate.epoch();
    gate.presentation_complete(second); // Duplicate acknowledgement.
    assert!(gate.accepts(admitted));
}

#[test]
fn unknown_contact_state_requires_an_authoritative_release() {
    let mut gate = Admission::new();
    let transition = gate.begin_transition();
    gate.presentation_complete(transition);
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn presented_screen_waits_for_reset_consumption() {
    let mut gate = released();
    let ticket = gate.begin_transition();
    gate.reset_requested();
    assert_eq!(gate.presentation_complete(ticket), None);
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(gate.epoch(), 0));
    assert_eq!(gate.resets_completed(1), Some(ticket));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn reset_consumed_before_scan_does_not_open_screen() {
    let mut gate = released();
    let ticket = gate.begin_transition();
    gate.reset_requested();
    assert_eq!(gate.resets_completed(1), None);
    assert!(!gate.accepts(gate.epoch()));
    assert_eq!(gate.presentation_complete(ticket), Some(ticket));
    assert!(gate.observe(gate.epoch(), 1));
}

#[test]
fn requests_arriving_during_reset_keep_gate_closed() {
    let mut gate = released();
    let ticket = gate.begin_transition();
    gate.reset_requested();
    gate.reset_requested();
    assert_eq!(gate.presentation_complete(ticket), None);
    gate.reset_requested();
    assert_eq!(gate.resets_completed(2), None);
    assert!(!gate.accepts(gate.epoch()));
    assert_eq!(gate.resets_completed(1), Some(ticket));
}

#[test]
fn reset_completion_cannot_complete_superseding_navigation() {
    let mut gate = released();
    let first = gate.begin_transition();
    gate.reset_requested();
    gate.presentation_complete(first);
    let second = gate.begin_transition();
    assert_eq!(gate.resets_completed(1), None);
    assert!(!gate.accepts(gate.epoch()));
    assert_eq!(gate.presentation_complete(first), None);
    assert_eq!(gate.presentation_complete(second), Some(second));
}

#[test]
fn held_contact_at_reset_completion_still_requires_release() {
    let mut gate = released();
    let ticket = gate.begin_transition();
    gate.reset_requested();
    gate.presentation_complete(ticket);
    assert!(!gate.observe(gate.epoch(), 1));
    assert_eq!(gate.resets_completed(1), Some(ticket));
    assert!(!gate.observe(gate.epoch(), 1));
    assert!(!gate.observe(gate.epoch(), 0));
    assert!(gate.observe(gate.epoch(), 1));
}
