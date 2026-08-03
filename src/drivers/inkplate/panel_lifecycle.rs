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
