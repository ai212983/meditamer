//! Provider actor mechanics shared by target wiring and real-driver host tests.
//! Credits cover ingress plus pending requests; control always travels separately.

use super::types::{
    BatteryFields, BatterySnapshot, BatteryStateSnapshot, BATTERY_PROVIDER_ID, FIELDS,
};
use crate::firmware::bounded_control::{Control, ControlAck, ControlCommand, ControlRequest};
use embassy_sync::blocking_mutex::raw::RawMutex;
use observation::{
    demand::Demand,
    ids::{ProviderGeneration, Revision},
    ingress::RequestIngress,
    runtime::{AcquisitionDriver, ProviderLoop, StepOutcome, SuspendOutcome},
    time::Instant,
};

pub(super) struct BatteryRuntime<
    'a,
    M: RawMutex,
    D: AcquisitionDriver<BatteryFields, BatterySnapshot>,
    const N: usize,
> {
    provider: ProviderLoop<BatteryFields, BatterySnapshot, D, N, FIELDS>,
    ingress: &'a RequestIngress<M, BatteryFields, N>,
    control: &'a Control<M>,
    last_sample_revision: Option<Revision>,
    stopped: bool,
}

impl<'a, M: RawMutex, D: AcquisitionDriver<BatteryFields, BatterySnapshot>, const N: usize>
    BatteryRuntime<'a, M, D, N>
{
    pub(super) fn new(
        driver: D,
        ingress: &'a RequestIngress<M, BatteryFields, N>,
        control: &'a Control<M>,
    ) -> Self {
        Self {
            provider: ProviderLoop::new(
                driver,
                BATTERY_PROVIDER_ID,
                ProviderGeneration::INITIAL,
                BatterySnapshot::default(),
            ),
            ingress,
            control,
            last_sample_revision: None,
            stopped: false,
        }
    }

    pub(super) fn state(&self) -> BatteryStateSnapshot {
        let state = self.provider.state();
        BatteryStateSnapshot {
            provider: BATTERY_PROVIDER_ID,
            generation: state.generation,
            revision: state.revision,
            health: state.health,
            last_attempt_at: state.last_attempt_at,
            last_sample_at: state.field_timestamps.get(0),
            last_sample_revision: self.last_sample_revision,
            snapshot: state.snapshot,
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.stopped
    }

    pub(super) async fn step(
        &mut self,
        demand: &Demand<BatteryFields, FIELDS>,
        now: impl Fn() -> Instant,
    ) -> StepOutcome<BatteryFields> {
        if self.stopped {
            return StepOutcome::RevisionExhausted;
        }
        let before = self.provider.pending_observe_now();
        let outcome = self.provider.step(demand, now).await;
        self.ingress
            .complete(before - self.provider.pending_observe_now());
        self.record_outcome(outcome);
        outcome
    }

    /// Commit actor lifecycle before callers can branch to control handling.
    pub(super) fn record_outcome(&mut self, outcome: StepOutcome<BatteryFields>) {
        match outcome {
            StepOutcome::Acquired { revision, .. } => self.last_sample_revision = Some(revision),
            StepOutcome::RevisionExhausted => {
                self.stopped = true;
                self.ingress.close();
            }
            _ => {}
        }
    }

    pub(super) fn drain_requests(
        &mut self,
        outcome: Option<StepOutcome<BatteryFields>>,
        now: Instant,
    ) {
        for _ in 0..N {
            let Ok(request) = self.ingress.try_receive() else {
                break;
            };
            let before = self.provider.pending_observe_now();
            let admitted = match outcome {
                Some(outcome) => self
                    .provider
                    .admit_observe_now_after_step(request, outcome, now),
                None => self.provider.admit_observe_now_at(request, now),
            };
            self.ingress
                .complete(before + 1 - self.provider.pending_observe_now());
            #[cfg(target_os = "none")]
            if let Err(reason) = admitted {
                console::println!(
                    "BATTERY_REQUEST rejected={:?} owner={} generation={}",
                    reason,
                    request.owner.0,
                    request.owner_generation.0
                );
            }
            #[cfg(not(target_os = "none"))]
            let _ = admitted;
        }
    }

    /// Retry cleanup for each current suspend; stale cleanup cannot authorize a
    /// newer request. Only a current resume reopens non-terminal acquisition.
    pub(super) async fn apply_control(
        &mut self,
        mut request: ControlRequest,
        now: impl Fn() -> Instant,
    ) {
        loop {
            if self.control.is_current(request) {
                match request.command {
                    ControlCommand::Suspend => {
                        self.ingress.close();
                        let pending = self.provider.pending_observe_now();
                        let outcome = self.provider.suspend(now()).await;
                        self.ingress.complete(pending);
                        #[cfg(target_os = "none")]
                        console::println!("BATTERY_SUSPEND outcome={:?}", outcome);
                        self.control.acknowledge(
                            request,
                            match outcome {
                                SuspendOutcome::Quiesced => ControlAck::Quiesced,
                                SuspendOutcome::CleanupFailed => ControlAck::CleanupFailed,
                            },
                        );
                    }
                    ControlCommand::Resume { .. } => {
                        if !self.stopped {
                            self.provider.resume();
                            if !self.ingress.open() {
                                #[cfg(target_os = "none")]
                                console::println!("BATTERY_REQUEST rejected=GenerationExhausted");
                            }
                        }
                        self.control.acknowledge(request, ControlAck::Running);
                        return;
                    }
                }
            }
            request = self.control.receive().await;
        }
    }
}
