use super::*;

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
    let result =
        merge_transaction_results::<(), _>(Err(TestError::Operation), Err(TestError::Shutdown));

    assert_eq!(result, Err(TestError::Operation));
}

#[test]
fn successful_operation_and_shutdown_succeed_as_one_transaction() {
    assert_eq!(
        merge_transaction_results::<_, TestError>(Ok(7), Ok(())),
        Ok(7)
    );
}

fn complete<F: core::future::Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut context = core::task::Context::from_waker(core::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        core::task::Poll::Ready(result) => result,
        core::task::Poll::Pending => panic!("test operation did not complete within its deadline"),
    }
}

struct TestPanel<'a> {
    state: PanelPowerState,
    shutdown_calls: usize,
    isolation_calls: usize,
    shutdown_error: bool,
    stall_after_lock: bool,
    shutdown_active: core::cell::Cell<bool>,
    shared_bus: &'a embassy_sync::mutex::Mutex<embassy_sync::blocking_mutex::raw::NoopRawMutex, ()>,
}

impl<'a> TestPanel<'a> {
    fn new(
        shared_bus: &'a embassy_sync::mutex::Mutex<
            embassy_sync::blocking_mutex::raw::NoopRawMutex,
            (),
        >,
    ) -> Self {
        Self {
            state: PanelPowerState::On,
            shutdown_calls: 0,
            isolation_calls: 0,
            shutdown_error: false,
            stall_after_lock: false,
            shutdown_active: core::cell::Cell::new(false),
            shared_bus,
        }
    }
}

struct ActiveShutdown<'a>(&'a core::cell::Cell<bool>);

impl Drop for ActiveShutdown<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl PanelShutdown for TestPanel<'_> {
    type Error = TestError;

    async fn shutdown_sequence(&mut self) -> Result<(), Self::Error> {
        self.shutdown_calls += 1;
        self.shutdown_active.set(true);
        let _active = ActiveShutdown(&self.shutdown_active);
        let _bus = self.shared_bus.lock().await;
        if self.stall_after_lock {
            core::future::pending::<()>().await;
        }
        if self.shutdown_error {
            return Err(InkplateHalError::I2c(TestError::Shutdown));
        }
        self.state.shutdown_finished(true);
        Ok(())
    }

    fn isolate_for_cleanup(&mut self) {
        assert!(
            !self.shutdown_active.get(),
            "shutdown must be dropped before isolation"
        );
        self.isolation_calls += 1;
        self.state.shutdown_finished(false);
    }
}

#[test]
fn failed_close_never_enters_shutdown_and_overrides_terminal_hold_or_waveform_error() {
    let bus = embassy_sync::mutex::Mutex::new(());
    for leave_on in [false, true] {
        for waveform_failed in [false, true] {
            let mut panel = TestPanel::new(&bus);
            let waveform = if waveform_failed {
                Err(InkplateHalError::I2c(TestError::Operation))
            } else {
                Ok(())
            };
            let result = complete(finalize_cooperative_transaction(
                &mut panel,
                waveform,
                leave_on,
                false,
                core::future::ready(()),
            ));
            assert!(matches!(
                result,
                Err(InkplateHalError::WaveformWindowNotQuiescent)
            ));
            assert_eq!(panel.shutdown_calls, 0);
            assert_eq!(panel.isolation_calls, 1);
            assert_eq!(panel.state, PanelPowerState::Starting);
        }
    }
}

#[test]
fn successful_close_preserves_normal_shutdown_and_terminal_hold() {
    let bus = embassy_sync::mutex::Mutex::new(());
    for leave_on in [false, true] {
        let mut panel = TestPanel::new(&bus);
        complete(finalize_cooperative_transaction(
            &mut panel,
            Ok(()),
            leave_on,
            true,
            core::future::pending(),
        ))
        .unwrap();
        assert_eq!(panel.isolation_calls, 0);
        assert_eq!(panel.shutdown_calls, usize::from(!leave_on));
        assert_eq!(
            panel.state,
            if leave_on {
                PanelPowerState::On
            } else {
                PanelPowerState::Off
            }
        );
    }
}

#[test]
fn held_shared_bus_bounds_finalization_and_retains_cleanup_until_recovery() {
    let bus = embassy_sync::mutex::Mutex::new(());
    let guard = bus.try_lock().unwrap();
    let mut panel = TestPanel::new(&bus);
    let result = complete(finalize_cooperative_transaction(
        &mut panel,
        Ok(()),
        false,
        true,
        core::future::ready(()),
    ));
    assert!(matches!(
        result,
        Err(InkplateHalError::PanelShutdownDeadline)
    ));
    assert_eq!(panel.isolation_calls, 1);
    assert_eq!(panel.state, PanelPowerState::Starting);

    drop(guard);
    complete(shutdown_before(&mut panel, core::future::pending())).unwrap();
    assert_eq!(panel.state, PanelPowerState::Off);
    assert_eq!(panel.shutdown_calls, 2);
}

#[test]
fn waveform_error_remains_primary_but_shutdown_failure_still_isolates_panel() {
    let bus = embassy_sync::mutex::Mutex::new(());
    let mut panel = TestPanel::new(&bus);
    panel.shutdown_error = true;
    let result = complete(finalize_cooperative_transaction(
        &mut panel,
        Err(InkplateHalError::I2c(TestError::Operation)),
        true,
        true,
        core::future::pending(),
    ));
    assert!(matches!(
        result,
        Err(InkplateHalError::I2c(TestError::Operation))
    ));
    assert_eq!(panel.shutdown_calls, 1);
    assert_eq!(panel.isolation_calls, 1);
    assert_eq!(panel.state, PanelPowerState::Starting);
}

#[test]
fn ordinary_shutdown_timeout_drops_an_inflight_transaction_and_releases_its_lock() {
    let bus = embassy_sync::mutex::Mutex::new(());
    let mut panel = TestPanel::new(&bus);
    panel.stall_after_lock = true;
    let result = complete(shutdown_before(&mut panel, core::future::ready(())));
    assert!(matches!(
        result,
        Err(InkplateHalError::PanelShutdownDeadline)
    ));
    assert_eq!(panel.isolation_calls, 1);
    assert_eq!(panel.state, PanelPowerState::Starting);
    assert!(
        bus.try_lock().is_ok(),
        "timed-out transaction retained the shared bus"
    );
}
