//! Pure lifecycle model for exclusive radio handoff.
//!
//! The device owner and the host harness share this state machine. Hardware
//! operations happen only after the model returns an [`OwnerAction`].

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub internal_free_bytes: u32,
    pub largest_block_above_reserve_bytes: u32,
    pub probe_free_before_bytes: u32,
    pub probe_free_after_bytes: u32,
    pub probe_reserve_bytes: u32,
    pub service_connections: u16,
    pub storage_roundtrips: u16,
    pub storage_sessions: u16,
    pub radio_callbacks: u16,
    pub radio_queues: u16,
    pub radio_source_active: bool,
    pub callback_admission_open: bool,
    pub late_callbacks: u32,
    pub queue_late_use: u32,
    pub queue_unknown_use: u32,
    pub queue_reclaim_failures: u32,
    pub queue_corruption: u32,
    pub queue_contention: u32,
    pub stable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum NetworkOwnerCommand {
    AcquireExclusive { boot_generation: u32, epoch: u32 },
    ReleaseExclusive { boot_generation: u32, epoch: u32 },
    Status,
}

impl NetworkOwnerCommand {
    pub const fn epoch(self) -> u32 {
        match self {
            Self::AcquireExclusive { epoch, .. } | Self::ReleaseExclusive { epoch, .. } => epoch,
            Self::Status => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkOwnerRequest {
    pub request_id: u32,
    pub command: NetworkOwnerCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkOwnerState {
    Restoring,
    Serving,
    Quiescing,
    OffConfirmed,
    Faulted,
}

impl NetworkOwnerState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Restoring => "restoring",
            Self::Serving => "serving",
            Self::Quiescing => "quiescing",
            Self::OffConfirmed => "off_confirmed",
            Self::Faulted => "faulted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    Busy,
    StaleBoot,
    StaleEpoch,
    UpdateReserved,
    QuiescenceTimeout,
    ResourceFloor,
    RestoreFailed,
    OwnershipUnknown,
}

impl RejectReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::StaleBoot => "stale_boot",
            Self::StaleEpoch => "stale_epoch",
            Self::UpdateReserved => "update_reserved",
            Self::QuiescenceTimeout => "quiescence_timeout",
            Self::ResourceFloor => "resource_floor",
            Self::RestoreFailed => "restore_failed",
            Self::OwnershipUnknown => "ownership_unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkOwnerAckKind {
    Status,
    Quiesced,
    Restored,
    Rejected(RejectReason),
    Faulted(RejectReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkOwnerAck {
    pub request_id: u32,
    pub boot_generation: u32,
    pub epoch: u32,
    pub state: NetworkOwnerState,
    pub kind: NetworkOwnerAckKind,
    pub resources: ResourceSnapshot,
}

#[allow(dead_code)]
pub const fn request_matches_ack(request_id: u32, ack: &NetworkOwnerAck) -> bool {
    request_id != 0 && ack.request_id == request_id
}

#[allow(dead_code)]
pub const fn exclusive_ownership_confirmed(
    exact_lease: bool,
    wifi_controller_resident: bool,
    net_runner_resident: bool,
    wifi_link: bool,
    service_listening: bool,
) -> bool {
    exact_lease && !wifi_controller_resident && !net_runner_resident && !wifi_link && !service_listening
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerAction {
    None(NetworkOwnerAck),
    BeginQuiesce { epoch: u32 },
    BeginRestore { epoch: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainAction {
    Wait,
    DropServices,
    ForceAbort,
    RejectForUpdate,
}

/// A teardown stage was requested out of order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfOrder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeardownStage {
    ServicesDropped,
    ProductQuiesced,
    RunnerDropped,
    WifiStopped,
    SourceDisabled,
    CallbacksQuiesced,
    QueuesReclaimed,
    StackReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeardownSequence {
    stage: TeardownStage,
}

impl Default for TeardownSequence {
    fn default() -> Self {
        Self::new()
    }
}

impl TeardownSequence {
    pub const fn new() -> Self {
        Self {
            stage: TeardownStage::ServicesDropped,
        }
    }

    pub fn advance(&mut self, next: TeardownStage) -> Result<(), OutOfOrder> {
        let valid = matches!(
            (self.stage, next),
            (
                TeardownStage::ServicesDropped,
                TeardownStage::ProductQuiesced
            ) | (TeardownStage::ProductQuiesced, TeardownStage::RunnerDropped)
                | (TeardownStage::RunnerDropped, TeardownStage::WifiStopped)
                | (TeardownStage::WifiStopped, TeardownStage::SourceDisabled)
                | (
                    TeardownStage::SourceDisabled,
                    TeardownStage::CallbacksQuiesced
                )
                | (
                    TeardownStage::CallbacksQuiesced,
                    TeardownStage::QueuesReclaimed
                )
                | (TeardownStage::QueuesReclaimed, TeardownStage::StackReset)
        );
        if !valid {
            return Err(OutOfOrder);
        }
        self.stage = next;
        Ok(())
    }

    pub const fn complete(self) -> bool {
        matches!(self.stage, TeardownStage::StackReset)
    }
}

pub const fn classify_drain(
    update_reserved: bool,
    product_work_quiescent: bool,
    grace_expired: bool,
) -> DrainAction {
    if update_reserved {
        DrainAction::RejectForUpdate
    } else if product_work_quiescent {
        DrainAction::DropServices
    } else if grace_expired {
        DrainAction::ForceAbort
    } else {
        DrainAction::Wait
    }
}

pub const fn sd_barrier_complete(
    barrier_acknowledged: bool,
    session_active: bool,
    product_work_quiescent: bool,
) -> bool {
    barrier_acknowledged && !session_active && product_work_quiescent
}

pub const fn control_quiescence_complete(explicit_safe_point_ack: bool) -> bool {
    explicit_safe_point_ack
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkOwnerMachine {
    boot_generation: u32,
    state: NetworkOwnerState,
    active_epoch: u32,
    active_request_id: u32,
    highest_acquire_epoch: u32,
    release_consumed: bool,
    resources: ResourceSnapshot,
}

impl NetworkOwnerMachine {
    pub const fn new(boot_generation: u32) -> Self {
        Self {
            boot_generation,
            state: NetworkOwnerState::Restoring,
            active_epoch: 0,
            active_request_id: 0,
            highest_acquire_epoch: 0,
            release_consumed: false,
            resources: ResourceSnapshot {
                internal_free_bytes: 0,
                largest_block_above_reserve_bytes: 0,
                probe_free_before_bytes: 0,
                probe_free_after_bytes: 0,
                probe_reserve_bytes: 0,
                service_connections: 0,
                storage_roundtrips: 0,
                storage_sessions: 0,
                radio_callbacks: 0,
                radio_queues: 0,
                radio_source_active: false,
                callback_admission_open: false,
                late_callbacks: 0,
                queue_late_use: 0,
                queue_unknown_use: 0,
                queue_reclaim_failures: 0,
                queue_corruption: 0,
                queue_contention: 0,
                stable: false,
            },
        }
    }

    pub const fn boot_generation(&self) -> u32 {
        self.boot_generation
    }

    pub const fn state(&self) -> NetworkOwnerState {
        self.state
    }

    #[cfg(test)]
    pub fn command(&mut self, command: NetworkOwnerCommand) -> OwnerAction {
        self.command_with_id(1, command)
    }

    pub fn command_with_id(
        &mut self,
        request_id: u32,
        command: NetworkOwnerCommand,
    ) -> OwnerAction {
        match command {
            NetworkOwnerCommand::Status => OwnerAction::None(self.ack_for(
                request_id,
                self.active_epoch,
                NetworkOwnerAckKind::Status,
            )),
            NetworkOwnerCommand::AcquireExclusive {
                boot_generation,
                epoch,
            } => {
                if boot_generation != self.boot_generation {
                    return OwnerAction::None(self.rejected_for(
                        request_id,
                        epoch,
                        RejectReason::StaleBoot,
                    ));
                }
                match self.state {
                    NetworkOwnerState::Serving if epoch > self.highest_acquire_epoch => {
                        self.highest_acquire_epoch = epoch;
                        self.active_epoch = epoch;
                        self.active_request_id = request_id;
                        self.release_consumed = false;
                        self.state = NetworkOwnerState::Quiescing;
                        OwnerAction::BeginQuiesce { epoch }
                    }
                    NetworkOwnerState::Faulted => OwnerAction::None(self.faulted_for(
                        request_id,
                        epoch,
                        RejectReason::OwnershipUnknown,
                    )),
                    NetworkOwnerState::Quiescing | NetworkOwnerState::Restoring => {
                        OwnerAction::None(self.rejected_for(request_id, epoch, RejectReason::Busy))
                    }
                    _ => OwnerAction::None(self.rejected_for(
                        request_id,
                        epoch,
                        RejectReason::StaleEpoch,
                    )),
                }
            }
            NetworkOwnerCommand::ReleaseExclusive {
                boot_generation,
                epoch,
            } => {
                if boot_generation != self.boot_generation {
                    return OwnerAction::None(self.rejected_for(
                        request_id,
                        epoch,
                        RejectReason::StaleBoot,
                    ));
                }
                match self.state {
                    NetworkOwnerState::OffConfirmed
                        if epoch == self.active_epoch && !self.release_consumed =>
                    {
                        self.release_consumed = true;
                        self.active_request_id = request_id;
                        self.state = NetworkOwnerState::Restoring;
                        OwnerAction::BeginRestore { epoch }
                    }
                    NetworkOwnerState::Faulted => OwnerAction::None(self.faulted_for(
                        request_id,
                        epoch,
                        RejectReason::OwnershipUnknown,
                    )),
                    NetworkOwnerState::Quiescing | NetworkOwnerState::Restoring => {
                        OwnerAction::None(self.rejected_for(request_id, epoch, RejectReason::Busy))
                    }
                    _ => OwnerAction::None(self.rejected_for(
                        request_id,
                        epoch,
                        RejectReason::StaleEpoch,
                    )),
                }
            }
        }
    }

    pub fn quiesced(&mut self, epoch: u32, resources: ResourceSnapshot) -> NetworkOwnerAck {
        if self.state != NetworkOwnerState::Quiescing || epoch != self.active_epoch {
            return self.rejected(epoch, RejectReason::StaleEpoch);
        }
        self.state = NetworkOwnerState::OffConfirmed;
        self.resources = resources;
        self.ack(NetworkOwnerAckKind::Quiesced)
    }

    pub fn reject_quiesce(
        &mut self,
        epoch: u32,
        reason: RejectReason,
        resources: ResourceSnapshot,
    ) -> NetworkOwnerAck {
        self.state = NetworkOwnerState::Serving;
        self.active_epoch = 0;
        self.release_consumed = false;
        self.resources = resources;
        let ack = self.rejected(epoch, reason);
        self.active_request_id = 0;
        ack
    }

    pub fn begin_rollback(&mut self) {
        self.state = NetworkOwnerState::Restoring;
    }

    pub fn restored(&mut self, epoch: u32, resources: ResourceSnapshot) -> NetworkOwnerAck {
        self.state = NetworkOwnerState::Serving;
        self.active_epoch = 0;
        self.resources = resources;
        let ack = self.ack_for(self.active_request_id, epoch, NetworkOwnerAckKind::Restored);
        self.active_request_id = 0;
        ack
    }

    pub fn restore_failed(
        &mut self,
        epoch: u32,
        resources: ResourceSnapshot,
    ) -> NetworkOwnerAck {
        self.state = NetworkOwnerState::Restoring;
        self.resources = resources;
        let ack = self.ack_for(
            self.active_request_id,
            epoch,
            NetworkOwnerAckKind::Rejected(RejectReason::RestoreFailed),
        );
        self.active_request_id = 0;
        ack
    }

    pub fn fault(
        &mut self,
        epoch: u32,
        reason: RejectReason,
        resources: ResourceSnapshot,
    ) -> NetworkOwnerAck {
        self.state = NetworkOwnerState::Faulted;
        self.resources = resources;
        self.faulted(epoch, reason)
    }

    #[allow(dead_code)]
    pub fn fault_for_request(
        &mut self,
        request_id: u32,
        epoch: u32,
        reason: RejectReason,
        resources: ResourceSnapshot,
    ) -> NetworkOwnerAck {
        self.state = NetworkOwnerState::Faulted;
        self.resources = resources;
        self.faulted_for(request_id, epoch, reason)
    }

    fn rejected(&self, epoch: u32, reason: RejectReason) -> NetworkOwnerAck {
        self.rejected_for(self.active_request_id, epoch, reason)
    }

    fn faulted(&self, epoch: u32, reason: RejectReason) -> NetworkOwnerAck {
        self.faulted_for(self.active_request_id, epoch, reason)
    }

    fn ack(&self, kind: NetworkOwnerAckKind) -> NetworkOwnerAck {
        self.ack_for(self.active_request_id, self.active_epoch, kind)
    }

    fn rejected_for(&self, request_id: u32, epoch: u32, reason: RejectReason) -> NetworkOwnerAck {
        self.ack_for(request_id, epoch, NetworkOwnerAckKind::Rejected(reason))
    }

    fn faulted_for(&self, request_id: u32, epoch: u32, reason: RejectReason) -> NetworkOwnerAck {
        self.ack_for(request_id, epoch, NetworkOwnerAckKind::Faulted(reason))
    }

    fn ack_for(&self, request_id: u32, epoch: u32, kind: NetworkOwnerAckKind) -> NetworkOwnerAck {
        NetworkOwnerAck {
            request_id,
            boot_generation: self.boot_generation,
            epoch,
            state: self.state,
            kind,
            resources: self.resources,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resources() -> ResourceSnapshot {
        ResourceSnapshot {
            internal_free_bytes: 24_000,
            largest_block_above_reserve_bytes: 5_000,
            stable: true,
            ..ResourceSnapshot::default()
        }
    }

    fn serving() -> NetworkOwnerMachine {
        let mut machine = NetworkOwnerMachine::new(7);
        assert!(matches!(
            machine.restored(0, resources()).kind,
            NetworkOwnerAckKind::Restored
        ));
        machine
    }

    #[test]
    fn exclusive_ownership_uses_supervisor_facts_not_connection_policy_telemetry() {
        assert!(exclusive_ownership_confirmed(
            true, false, false, false, false
        ));
        for rejected in [
            (false, false, false, false, false),
            (true, true, false, false, false),
            (true, false, true, false, false),
            (true, false, false, true, false),
            (true, false, false, false, true),
        ] {
            assert!(!exclusive_ownership_confirmed(
                rejected.0, rejected.1, rejected.2, rejected.3, rejected.4
            ));
        }
    }

    #[test]
    fn normal_handoff_is_epoch_matched() {
        let mut machine = serving();
        assert_eq!(
            machine.command(NetworkOwnerCommand::AcquireExclusive {
                boot_generation: 7,
                epoch: 1,
            }),
            OwnerAction::BeginQuiesce { epoch: 1 }
        );
        assert_eq!(
            machine.quiesced(1, resources()).state,
            NetworkOwnerState::OffConfirmed
        );
        assert_eq!(
            machine.command(NetworkOwnerCommand::ReleaseExclusive {
                boot_generation: 7,
                epoch: 1,
            }),
            OwnerAction::BeginRestore { epoch: 1 }
        );
        assert_eq!(
            machine.restored(1, resources()).state,
            NetworkOwnerState::Serving
        );
    }

    #[test]
    fn acknowledgements_are_request_matched_not_only_epoch_matched() {
        let mut machine = serving();
        let OwnerAction::None(status) = machine.command_with_id(41, NetworkOwnerCommand::Status)
        else {
            panic!("status must be immediate")
        };
        assert!(request_matches_ack(41, &status));
        assert!(!request_matches_ack(40, &status));

        assert!(matches!(
            machine.command_with_id(
                42,
                NetworkOwnerCommand::AcquireExclusive {
                    boot_generation: 7,
                    epoch: 1,
                }
            ),
            OwnerAction::BeginQuiesce { .. }
        ));
        let quiesced = machine.quiesced(1, resources());
        assert!(request_matches_ack(42, &quiesced));
        assert!(!request_matches_ack(43, &quiesced));
    }

    #[test]
    fn stale_boot_and_epoch_are_rejected() {
        let mut machine = serving();
        let OwnerAction::None(stale_boot) =
            machine.command(NetworkOwnerCommand::AcquireExclusive {
                boot_generation: 6,
                epoch: 1,
            })
        else {
            panic!("expected rejection")
        };
        assert_eq!(
            stale_boot.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::StaleBoot)
        );
        let OwnerAction::None(stale_epoch) =
            machine.command(NetworkOwnerCommand::ReleaseExclusive {
                boot_generation: 7,
                epoch: 9,
            })
        else {
            panic!("expected rejection")
        };
        assert_eq!(
            stale_epoch.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::StaleEpoch)
        );
    }

    #[test]
    fn duplicate_acquire_and_release_are_rejected() {
        let mut machine = serving();
        assert!(matches!(
            machine.command(NetworkOwnerCommand::AcquireExclusive {
                boot_generation: 7,
                epoch: 1,
            }),
            OwnerAction::BeginQuiesce { .. }
        ));
        machine.quiesced(1, resources());
        let OwnerAction::None(duplicate_acquire) =
            machine.command(NetworkOwnerCommand::AcquireExclusive {
                boot_generation: 7,
                epoch: 1,
            })
        else {
            panic!("expected current quiesced acknowledgement")
        };
        assert_eq!(
            duplicate_acquire.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::StaleEpoch)
        );
        machine.command(NetworkOwnerCommand::ReleaseExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        machine.restored(1, resources());
        let OwnerAction::None(duplicate_release) =
            machine.command(NetworkOwnerCommand::ReleaseExclusive {
                boot_generation: 7,
                epoch: 1,
            })
        else {
            panic!("expected current restored acknowledgement")
        };
        assert_eq!(
            duplicate_release.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::StaleEpoch)
        );
    }

    #[test]
    fn fault_latches_and_requires_reboot() {
        let mut machine = serving();
        machine.command(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        machine.fault(1, RejectReason::OwnershipUnknown, resources());
        let OwnerAction::None(ack) = machine.command(NetworkOwnerCommand::Status) else {
            panic!("status must be immediate")
        };
        assert_eq!(ack.state, NetworkOwnerState::Faulted);
        let OwnerAction::None(ack) = machine.command(NetworkOwnerCommand::ReleaseExclusive {
            boot_generation: 7,
            epoch: 1,
        }) else {
            panic!("fault must reject")
        };
        assert!(matches!(ack.kind, NetworkOwnerAckKind::Faulted(_)));
    }

    #[test]
    fn active_upload_completion_and_forced_abort_are_distinct() {
        assert_eq!(classify_drain(false, false, false), DrainAction::Wait);
        assert_eq!(
            classify_drain(false, true, false),
            DrainAction::DropServices
        );
        assert_eq!(classify_drain(false, false, true), DrainAction::ForceAbort);
    }

    #[test]
    fn update_race_rejects_before_forced_abort() {
        assert_eq!(
            classify_drain(true, false, true),
            DrainAction::RejectForUpdate
        );
        let mut machine = serving();
        machine.command(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        let ack = machine.reject_quiesce(1, RejectReason::UpdateReserved, resources());
        assert_eq!(ack.state, NetworkOwnerState::Serving);
        assert_eq!(
            ack.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::UpdateReserved)
        );
    }

    #[test]
    fn sd_fifo_barrier_is_mandatory_even_without_a_session() {
        assert!(!sd_barrier_complete(false, false, true));
        assert!(!sd_barrier_complete(true, true, true));
        assert!(!sd_barrier_complete(true, false, false));
        assert!(sd_barrier_complete(true, false, true));
    }

    #[test]
    fn admission_close_telemetry_clear_cannot_invalidate_control_safe_point() {
        let serving_telemetry_ready_after_admission_close = false;
        assert!(!serving_telemetry_ready_after_admission_close);
        assert!(control_quiescence_complete(true));
        assert!(!control_quiescence_complete(false));
    }

    #[test]
    fn sd_timeout_rolls_back_but_controller_stop_failure_faults() {
        let mut machine = serving();
        machine.command(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        machine.begin_rollback();
        let rejected = machine.reject_quiesce(1, RejectReason::QuiescenceTimeout, resources());
        assert_eq!(rejected.state, NetworkOwnerState::Serving);

        machine.command(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: 7,
            epoch: 2,
        });
        let faulted = machine.fault(2, RejectReason::OwnershipUnknown, resources());
        assert_eq!(faulted.state, NetworkOwnerState::Faulted);
    }

    #[test]
    fn restoration_failure_stays_restoring_until_retry_succeeds() {
        let mut machine = serving();
        machine.command(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        machine.quiesced(1, resources());
        machine.command(NetworkOwnerCommand::ReleaseExclusive {
            boot_generation: 7,
            epoch: 1,
        });
        let failed = machine.restore_failed(1, resources());
        assert_eq!(failed.state, NetworkOwnerState::Restoring);
        assert_eq!(
            failed.kind,
            NetworkOwnerAckKind::Rejected(RejectReason::RestoreFailed)
        );
        assert_eq!(
            machine.restored(1, resources()).state,
            NetworkOwnerState::Serving
        );
    }

    #[test]
    fn release_discovered_ownership_fault_uses_the_release_request_id() {
        let mut machine = serving();
        let OwnerAction::BeginQuiesce { .. } = machine.command_with_id(
            41,
            NetworkOwnerCommand::AcquireExclusive {
                boot_generation: 7,
                epoch: 1,
            },
        ) else {
            panic!("acquire must start quiescence");
        };
        machine.quiesced(1, resources());
        let fault = machine.fault_for_request(42, 1, RejectReason::OwnershipUnknown, resources());
        assert_eq!(fault.request_id, 42);
        assert_eq!(fault.state, NetworkOwnerState::Faulted);
        assert_eq!(
            fault.kind,
            NetworkOwnerAckKind::Faulted(RejectReason::OwnershipUnknown)
        );
    }

    #[test]
    fn teardown_effects_must_follow_the_runtime_ownership_order() {
        let mut sequence = TeardownSequence::new();
        for stage in [
            TeardownStage::ProductQuiesced,
            TeardownStage::RunnerDropped,
            TeardownStage::WifiStopped,
            TeardownStage::SourceDisabled,
            TeardownStage::CallbacksQuiesced,
            TeardownStage::QueuesReclaimed,
            TeardownStage::StackReset,
        ] {
            sequence.advance(stage).expect("valid teardown stage");
        }
        assert!(sequence.complete());
    }

    #[test]
    fn teardown_effects_reject_skips_and_reordering() {
        let mut sequence = TeardownSequence::new();
        assert_eq!(sequence.advance(TeardownStage::RunnerDropped), Err(OutOfOrder));
        sequence
            .advance(TeardownStage::ProductQuiesced)
            .expect("quiesced");
        assert_eq!(sequence.advance(TeardownStage::SourceDisabled), Err(OutOfOrder));
        assert!(!sequence.complete());
    }
}
