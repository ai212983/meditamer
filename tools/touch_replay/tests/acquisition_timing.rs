#[path = "../../../src/firmware/touch/tasks/acquisition/state.rs"]
mod acquisition_state;

#[test]
fn recovery_poll_is_slow_and_not_a_release_deadline() {
    assert_eq!(acquisition_state::IDLE_RECOVERY_MS, 250);
}

#[test]
fn active_contact_uses_fast_polling_until_explicit_release() {
    let mut state = acquisition_state::ContactSamplingState::new();
    assert_eq!(state.poll_interval_ms(), 250);

    state.record_authoritative_count(1);
    assert_eq!(state.poll_interval_ms(), 8);

    state.record_authoritative_count(0);
    assert_eq!(state.poll_interval_ms(), 250);
}

#[test]
fn non_report_does_not_change_contact_sampling_state() {
    let mut state = acquisition_state::ContactSamplingState::new();
    state.record_authoritative_count(1);

    assert_eq!(acquisition_state::authoritative_touch_count(0x55, 0), None);
    assert_eq!(state.poll_interval_ms(), 8);
}

#[test]
fn explicit_zero_contact_report_is_a_release() {
    assert_eq!(acquisition_state::authoritative_touch_count(0x5A, 0), Some(0));
}

#[test]
fn active_contact_report_remains_active() {
    assert_eq!(acquisition_state::authoritative_touch_count(0x5A, 1), Some(1));
}

#[test]
fn non_touch_packet_cannot_synthesize_release() {
    assert_eq!(acquisition_state::authoritative_touch_count(0x55, 0), None);
}
