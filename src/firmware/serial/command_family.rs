use core::fmt::Write;

use crate::firmware::{
    serial::{command_dispatch, commands::SerialCommand, task_state::SerialTaskState},
    touch::debug_log::uart_write_all,
    types::SerialUart,
    update as firmware_update,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreBoxCommandClass {
    Firmware,
    Metrics,
    MetricsNet,
    Ordinary,
}

pub(super) const fn pre_box_command_class(cmd: &SerialCommand) -> PreBoxCommandClass {
    match cmd {
        SerialCommand::FirmwareFactoryBoot => PreBoxCommandClass::Firmware,
        SerialCommand::Metrics => PreBoxCommandClass::Metrics,
        SerialCommand::MetricsNet => PreBoxCommandClass::MetricsNet,
        _ => PreBoxCommandClass::Ordinary,
    }
}

pub(super) const fn firmware_command_tag(cmd: &SerialCommand) -> &'static str {
    match cmd {
        SerialCommand::FirmwareFactoryBoot => "FWFACTORYBOOT",
        _ => panic!("only allocated firmware commands have a firmware tag"),
    }
}

pub(super) async fn fail_firmware_dispatch_allocation(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    command_tag: &str,
) {
    let update_grant_active =
        state.firmware_update_hardware_lease_active() || firmware_update::transport_quiet();
    if update_grant_active {
        // This path must not allocate: it is the recovery boundary for the allocation that failed.
        if state.firmware_update_hardware_lease_active() {
            command_dispatch::release_firmware_update_hardware(state).await;
        }
        firmware_update::end_transport();
    }
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        &mut line,
        "{} ERR reason=internal_dispatch_alloc update_aborted={}\r\n",
        command_tag,
        if update_grant_active { "yes" } else { "no" },
    );
    let _ = uart_write_all(uart, line.as_bytes()).await;
}

const _: () = {
    assert!(matches!(
        pre_box_command_class(&SerialCommand::FirmwareFactoryBoot),
        PreBoxCommandClass::Firmware
    ));
    assert!(matches!(
        pre_box_command_class(&SerialCommand::Metrics),
        PreBoxCommandClass::Metrics
    ));
    assert!(matches!(
        pre_box_command_class(&SerialCommand::MetricsNet),
        PreBoxCommandClass::MetricsNet
    ));
    assert!(matches!(
        pre_box_command_class(&SerialCommand::Ping),
        PreBoxCommandClass::Ordinary
    ));
};
