// Every item here is called only from the hardware-gated display transaction
// path (or, below, from this module's own tests): on a host build with that
// path excluded, nothing outside `#[cfg(test)]` calls in, and it would
// otherwise read as dead code.
#![cfg_attr(not(any(test, target_os = "none")), allow(dead_code))]

#[cfg(test)]
#[path = "panel_lifecycle/tests.rs"]
mod tests;

use core::future::{poll_fn, Future};
use core::task::Poll;

use crate::{InkplateHalError, Result};

/// Covers expander-lock admission, shared-I2C admission, transactions, and rail
/// settling together. The 250ms power-good poll fits within this total budget.
#[cfg(target_os = "none")]
pub(crate) const PANEL_SHUTDOWN_BUDGET_MS: u64 = 2_000;

/// Hardware operations used by the production finalization path. The abort
/// operation must be synchronous and must not touch shared I2C or the expander.
pub(crate) trait PanelShutdown {
    type Error;

    async fn shutdown_sequence(&mut self) -> Result<(), Self::Error>;
    fn isolate_for_cleanup(&mut self);
}

/// The deadline also covers waiting for a lock held by an unresponsive client.
/// Drop the shutdown future before isolating GPIOs so no outstanding operation
/// can subsequently resume and change the panel state.
pub(crate) async fn shutdown_before<P: PanelShutdown>(
    panel: &mut P,
    deadline: impl Future<Output = ()>,
) -> Result<(), P::Error> {
    let result = {
        let mut shutdown = core::pin::pin!(panel.shutdown_sequence());
        let mut deadline = core::pin::pin!(deadline);
        poll_fn(|cx| {
            if let Poll::Ready(result) = shutdown.as_mut().poll(cx) {
                return Poll::Ready(result);
            }
            if deadline.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Err(InkplateHalError::PanelShutdownDeadline));
            }
            Poll::Pending
        })
        .await
    };
    if result.is_err() {
        panel.isolate_for_cleanup();
    }
    result
}

/// Shared by full, partial and gate-drain cooperative refreshes, including full
/// fallback. Failed closure forbids even attempting normal PMIC finalization.
pub(crate) async fn finalize_cooperative_transaction<P: PanelShutdown>(
    panel: &mut P,
    operation: Result<(), P::Error>,
    leave_on: bool,
    window_closed: bool,
    deadline: impl Future<Output = ()>,
) -> Result<(), P::Error> {
    if !window_closed {
        panel.isolate_for_cleanup();
        return Err(InkplateHalError::WaveformWindowNotQuiescent);
    }
    match display_transaction_finalization(operation.is_ok(), leave_on) {
        DisplayTransactionFinalization::ParkPoweredPanel => operation,
        DisplayTransactionFinalization::ShutDownPanel => {
            let shutdown = shutdown_before(panel, deadline).await;
            merge_transaction_results(operation, shutdown)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelPowerState {
    Off,
    Starting,
    On,
}

impl PanelPowerState {
    pub(crate) const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }

    pub(crate) const fn requires_shutdown(self) -> bool {
        !matches!(self, Self::Off)
    }

    pub(crate) fn begin_startup(&mut self) {
        *self = Self::Starting;
    }

    pub(crate) fn startup_succeeded(&mut self) {
        debug_assert!(matches!(self, Self::Starting));
        *self = Self::On;
    }

    pub(crate) fn shutdown_finished(&mut self, succeeded: bool) {
        *self = if succeeded { Self::Off } else { Self::Starting };
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DisplayTransactionFinalization {
    ParkPoweredPanel,
    ShutDownPanel,
}

pub(crate) const fn display_transaction_finalization(
    operation_succeeded: bool,
    leave_on: bool,
) -> DisplayTransactionFinalization {
    if operation_succeeded && leave_on {
        DisplayTransactionFinalization::ParkPoweredPanel
    } else {
        DisplayTransactionFinalization::ShutDownPanel
    }
}

/// Retains the operation error when both the waveform and its mandatory
/// shutdown fail. The panel power state remains `Starting` after a failed
/// shutdown so the next transaction retries recovery instead of assuming the
/// hardware is off.
pub(crate) fn merge_transaction_results<T, E>(
    operation: core::result::Result<T, E>,
    shutdown: core::result::Result<(), E>,
) -> core::result::Result<T, E> {
    match operation {
        Ok(value) => shutdown.map(|()| value),
        Err(error) => Err(error),
    }
}
