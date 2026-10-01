use super::{command_family, commands::SerialCommand};

/// Synchronous dispatch plan for a parsed serial command.
///
/// Computed by borrow without retaining any future state, so the single
/// async dispatch point in `super::handle_uart_byte` owns exactly one
/// `SerialCommand` and awaits leaf handlers directly. This preserves the
/// original task-pool layout: no nested async frame may retain a duplicate
/// command or line buffer.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum DispatchRoute {
    ObservationFixture,
    ObservationPanelCycle,
    Firmware,
    MetricsGeneral,
    MetricsNetwork,
    #[cfg(feature = "firmware-trace")]
    Trace,
    LowOverhead,
    Ordinary,
}

pub(super) fn route_for_command(cmd: &SerialCommand) -> DispatchRoute {
    if matches!(cmd, SerialCommand::ObservationFixture(_)) {
        return DispatchRoute::ObservationFixture;
    }
    if matches!(cmd, SerialCommand::ObservationPanelCycle(_)) {
        return DispatchRoute::ObservationPanelCycle;
    }
    if command_family::pre_box_command_class(cmd) == command_family::PreBoxCommandClass::Firmware {
        return DispatchRoute::Firmware;
    }
    match command_family::pre_box_command_class(cmd) {
        command_family::PreBoxCommandClass::Metrics => {
            return DispatchRoute::MetricsGeneral;
        }
        command_family::PreBoxCommandClass::MetricsNet => {
            return DispatchRoute::MetricsNetwork;
        }
        command_family::PreBoxCommandClass::Firmware
        | command_family::PreBoxCommandClass::Ordinary => {}
    }
    #[cfg(feature = "firmware-trace")]
    if matches!(cmd, SerialCommand::Trace { .. }) {
        return DispatchRoute::Trace;
    }
    if is_low_overhead_diagnostic(cmd) {
        return DispatchRoute::LowOverhead;
    }
    DispatchRoute::Ordinary
}

pub(super) fn metrics_busy_line(route: DispatchRoute) -> &'static [u8] {
    match route {
        DispatchRoute::MetricsNetwork => b"METRICSNET BUSY\r\n",
        _ => b"METRICS BUSY\r\n",
    }
}

pub(super) fn is_low_overhead_diagnostic(cmd: &SerialCommand) -> bool {
    #[cfg(feature = "ble-foundation")]
    if matches!(
        cmd,
        SerialCommand::StackStatus
            | SerialCommand::ConsoleStats
            | SerialCommand::MountainStatus
            | SerialCommand::AllocatorStatus
            | SerialCommand::BlePhase1sStart { .. }
            | SerialCommand::BlePhase1sStatus
            | SerialCommand::RadioHandoffAcquire { .. }
            | SerialCommand::RadioHandoffRelease { .. }
            | SerialCommand::RadioHandoffStatus
    ) {
        return true;
    }
    #[cfg(not(feature = "ble-foundation"))]
    if matches!(
        cmd,
        SerialCommand::StackStatus
            | SerialCommand::ConsoleStats
            | SerialCommand::MountainStatus
            | SerialCommand::AllocatorStatus
    ) {
        return true;
    }
    #[cfg(feature = "asset-upload-http")]
    if matches!(
        cmd,
        SerialCommand::NetStatus
            | SerialCommand::StateSet { .. }
            | SerialCommand::StateDiag { .. }
            | SerialCommand::NetStart
            | SerialCommand::NetStop
            | SerialCommand::NetRecover
    ) {
        return true;
    }
    false
}
