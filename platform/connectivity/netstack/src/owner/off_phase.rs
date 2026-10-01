//! Radio-off service and restore handoff phase.

use super::*;

pub(super) enum OffWaitResult {
    InitialCompleted,
    Restore(u32),
}

#[derive(Clone, Copy)]
pub(super) enum OffPhase {
    Initial,
    Confirmed(u32),
}

pub(super) async fn drive_off_phase<S: RadioOffService>(
    machine: &mut NetworkOwnerMachine,
    off_service: &mut S,
    phase: OffPhase,
) -> OffWaitResult {
    let (epoch, initial) = match phase {
        OffPhase::Initial => (0, true),
        OffPhase::Confirmed(epoch) => (epoch, false),
    };
    loop {
        let mut service = pin!(off_service.run(machine.boot_generation(), epoch));
        if initial {
            service.await;
            return OffWaitResult::InitialCompleted;
        }
        loop {
            let request = match select(service.as_mut(), HANDOFF_COMMANDS.receive()).await {
                Either::First(()) => break,
                Either::Second(request) => request,
            };
            #[cfg(feature = "ble-foundation")]
            if matches!(
                request.command,
                NetworkOwnerCommand::ReleaseExclusive { .. }
            ) {
                match arbitration::claim::ble_ownership() {
                    arbitration::claim::Ownership::Unknown => {
                        clear_exclusive_lease();
                        let ack = machine.fault_for_request(
                            request.request_id,
                            request.command.epoch(),
                            RejectReason::OwnershipUnknown,
                            resource_snapshot(false),
                        );
                        publish_ack(ack);
                        println_ack(ack);
                        loop {
                            let request = HANDOFF_COMMANDS.receive().await;
                            let OwnerAction::None(ack) =
                                machine.command_with_id(request.request_id, request.command)
                            else {
                                unreachable!()
                            };
                            publish_ack(ack);
                        }
                    }
                    arbitration::claim::Ownership::Active => {
                        let ack = reject_without_transition(
                            machine,
                            request.request_id,
                            request.command.epoch(),
                            RejectReason::Busy,
                        );
                        publish_ack(ack);
                        continue;
                    }
                    arbitration::claim::Ownership::KnownClosed => {}
                }
            }
            match machine.command_with_id(request.request_id, request.command) {
                // OffConfirmed status reports the coherent snapshot that granted
                // the lease. Re-probing here would reopen the deferred-free race
                // after the binding measurement has already passed.
                OwnerAction::None(ack) => publish_ack(ack),
                OwnerAction::BeginRestore { epoch } => {
                    #[cfg(feature = "ble-foundation")]
                    clear_exclusive_lease();
                    return OffWaitResult::Restore(epoch);
                }
                OwnerAction::BeginQuiesce { .. } => unreachable!(),
            }
        }
    }
}

pub(super) async fn service_restore_wait(machine: &mut NetworkOwnerMachine, ack: NetworkOwnerAck) {
    publish_ack(ack);
    match select(
        HANDOFF_COMMANDS.receive(),
        Timer::after(Duration::from_secs(2)),
    )
    .await
    {
        Either::First(request) => {
            let OwnerAction::None(ack) =
                machine.command_with_id(request.request_id, request.command)
            else {
                return;
            };
            publish_ack(ack);
        }
        Either::Second(_) => {}
    }
}

pub(super) async fn fault_holding_owners(
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
    reason: RejectReason,
    controller: WifiController<'_>,
) -> ! {
    let _controller = controller;
    let ack = machine.fault(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
    println_ack(ack);
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        let OwnerAction::None(ack) = machine.command_with_id(request.request_id, request.command)
        else {
            unreachable!()
        };
        publish_ack(ack);
    }
}

pub(super) async fn fault_holding_peripheral(
    machine: &mut NetworkOwnerMachine,
    epoch: u32,
    reason: RejectReason,
) -> ! {
    let ack = machine.fault(epoch, reason, resource_snapshot(false));
    publish_ack(ack);
    println_ack(ack);
    loop {
        let request = HANDOFF_COMMANDS.receive().await;
        let OwnerAction::None(ack) = machine.command_with_id(request.request_id, request.command)
        else {
            unreachable!()
        };
        publish_ack(ack);
    }
}

pub(super) fn reject_without_transition(
    machine: &mut NetworkOwnerMachine,
    request_id: u32,
    epoch: u32,
    reason: RejectReason,
) -> NetworkOwnerAck {
    NetworkOwnerAck {
        request_id,
        boot_generation: machine.boot_generation(),
        epoch,
        state: machine.state(),
        kind: NetworkOwnerAckKind::Rejected(reason),
        resources: resource_snapshot(false),
    }
}
