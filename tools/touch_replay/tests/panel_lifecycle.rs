#[path = "../../../src/drivers/inkplate/panel_lifecycle.rs"]
mod panel_lifecycle;

use panel_lifecycle::{
    display_transaction_finalization, merge_transaction_results, DisplayTransactionFinalization,
    PanelPowerState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestError {
    Operation,
    Shutdown,
}

#[test]
fn only_confirmed_off_state_skips_shutdown() {
    assert!(!PanelPowerState::Off.requires_shutdown());
    assert!(PanelPowerState::Starting.requires_shutdown());
    assert!(PanelPowerState::On.requires_shutdown());
}

#[test]
fn only_confirmed_on_state_is_on() {
    assert!(!PanelPowerState::Off.is_on());
    assert!(!PanelPowerState::Starting.is_on());
    assert!(PanelPowerState::On.is_on());
}

#[test]
fn successful_startup_moves_through_starting_to_on() {
    let mut state = PanelPowerState::Off;

    state.begin_startup();
    assert_eq!(state, PanelPowerState::Starting);
    state.startup_succeeded();
    assert_eq!(state, PanelPowerState::On);
}

#[test]
fn failed_shutdown_remains_recoverable_and_successful_retry_reaches_off() {
    let mut state = PanelPowerState::On;

    state.shutdown_finished(false);
    assert_eq!(state, PanelPowerState::Starting);
    state.shutdown_finished(true);
    assert_eq!(state, PanelPowerState::Off);
}

#[test]
fn successful_leave_on_transaction_parks_panel() {
    assert_eq!(
        display_transaction_finalization(true, true),
        DisplayTransactionFinalization::ParkPoweredPanel
    );
}

#[test]
fn successful_default_transaction_shuts_panel_down() {
    assert_eq!(
        display_transaction_finalization(true, false),
        DisplayTransactionFinalization::ShutDownPanel
    );
}

#[test]
fn failed_transaction_always_shuts_down_even_when_leave_on_was_requested() {
    for leave_on in [false, true] {
        assert_eq!(
            display_transaction_finalization(false, leave_on),
            DisplayTransactionFinalization::ShutDownPanel
        );
    }
}

#[test]
fn shutdown_error_is_returned_after_successful_operation() {
    let result = merge_transaction_results::<(), _>(Ok(()), Err(TestError::Shutdown));

    assert_eq!(result, Err(TestError::Shutdown));
}

#[test]
fn operation_error_is_preserved_when_shutdown_also_fails() {
    let result = merge_transaction_results::<(), _>(
        Err(TestError::Operation),
        Err(TestError::Shutdown),
    );

    assert_eq!(result, Err(TestError::Operation));
}

#[test]
fn successful_operation_and_shutdown_succeed_as_one_transaction() {
    assert_eq!(merge_transaction_results::<_, TestError>(Ok(7), Ok(())), Ok(7));
}
