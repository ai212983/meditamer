//! Correlated, persistent desired-state control for panel-bus participants.
//!
//! A request supersedes pending older intent; it is not a FIFO operation.
//! In particular, publishing Running cannot fail because a command queue is
//! full. Timing out only stops the caller waiting, not the participant's
//! eventual convergence. Acknowledgements belong to the exact current request.

use core::{
    cell::RefCell,
    future::Future,
    sync::atomic::{AtomicU32, Ordering},
};

use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};

pub(crate) async fn bounded<Op, Deadline, T>(operation: Op, deadline: Deadline) -> Option<T>
where
    Op: Future<Output = T>,
    Deadline: Future<Output = ()>,
{
    match select(operation, deadline).await {
        Either::First(value) => Some(value),
        Either::Second(()) => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendAck {
    Quiesced,
    CleanupFailed,
    TimedOut,
    Superseded,
    Exhausted,
}

impl SuspendAck {
    pub(crate) const fn permits_shared_bus_access(self) -> bool {
        matches!(self, Self::Quiesced)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlCommand {
    Suspend,
    Resume { reset_pipeline: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlRequest {
    id: u32,
    pub command: ControlCommand,
}

impl ControlRequest {
    pub(crate) const fn id(self) -> u32 {
        self.id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlAck {
    Quiesced,
    CleanupFailed,
    Running,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaitOutcome {
    Acknowledged(ControlAck),
    Superseded,
    TimedOut,
}

struct State {
    current: ControlRequest,
    acknowledgement: Option<ControlAck>,
    reset_pending: bool,
}

/// One participant, one receiver, and one normally active acknowledged caller.
/// Fire-and-forget publishers may supersede that caller. The current state is
/// retained independently of both wake signals, so dropped waits lose no intent.
/// u32::MAX is a terminal Running-only epoch: exhaustion rejects new suspension
/// while still allowing recovery. No suspension request identity is ever reused.
pub(crate) struct Control<M: RawMutex> {
    state: Mutex<M, RefCell<State>>,
    published_id: AtomicU32,
    requests: Signal<M, ControlRequest>,
    changed: Signal<M, ()>,
}

impl<M: RawMutex> Control<M> {
    /// Wait for publication without making control depend on a consumer.
    /// The caller must handle/acknowledge the returned request before resuming work.
    pub(crate) async fn interruptible<T>(
        &self,
        work: impl Future<Output = T>,
    ) -> Result<T, ControlRequest> {
        match select(self.receive(), work).await {
            Either::First(request) => Err(request),
            Either::Second(value) => Ok(value),
        }
    }

    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                current: ControlRequest {
                    id: 0,
                    command: ControlCommand::Resume {
                        reset_pipeline: false,
                    },
                },
                acknowledgement: None,
                reset_pending: false,
            })),
            published_id: AtomicU32::new(0),
            requests: Signal::new(),
            changed: Signal::new(),
        }
    }

    pub(crate) fn request_suspend(&self) -> Option<ControlRequest> {
        self.publish(ControlCommand::Suspend)
    }

    /// Admission is synchronous and cannot be blocked by old requests.
    pub(crate) fn request_resume(&self, reset_pipeline: bool) -> ControlRequest {
        self.publish(ControlCommand::Resume { reset_pipeline })
            .expect("resume is always admitted")
    }

    fn publish(&self, command: ControlCommand) -> Option<ControlRequest> {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            let id = state.current.id.saturating_add(1);
            if id == u32::MAX && command == ControlCommand::Suspend {
                return None;
            }
            let command = match command {
                ControlCommand::Resume { reset_pipeline } => {
                    state.reset_pending |= reset_pipeline;
                    ControlCommand::Resume {
                        reset_pipeline: state.reset_pending,
                    }
                }
                ControlCommand::Suspend => ControlCommand::Suspend,
            };
            let request = ControlRequest { id, command };
            state.current = request;
            state.acknowledgement = None;
            self.published_id.store(id, Ordering::Release);
            // Publish under the same lock as the identity update, preventing
            // concurrent publishers from reversing desired-state order.
            self.requests.signal(request);
            self.changed.signal(());
            Some(request)
        })
    }

    /// One locked view: a newer intent never inherits the previous acknowledgement.
    pub(crate) fn snapshot(&self) -> (ControlRequest, Option<ControlAck>) {
        self.state.lock(|cell| {
            let state = cell.borrow();
            (state.current, state.acknowledgement)
        })
    }

    pub(crate) fn is_current(&self, request: ControlRequest) -> bool {
        if self.current_id() != request.id() {
            return false;
        }
        self.state.lock(|cell| cell.borrow().current == request)
    }

    /// Hard-park diagnostics must observe new intent with interrupts disabled,
    /// without entering a mutex or relying on executor wakeups.
    #[inline(always)]
    pub(crate) fn current_id(&self) -> u32 {
        self.published_id.load(Ordering::Acquire)
    }

    pub(crate) fn try_receive(&self) -> Option<ControlRequest> {
        while let Some(request) = self.requests.try_take() {
            if self.is_current(request) {
                return Some(request);
            }
        }
        None
    }

    pub(crate) async fn receive(&self) -> ControlRequest {
        loop {
            let request = self.requests.wait().await;
            if self.is_current(request) {
                return request;
            }
        }
    }

    /// Called only after the participant has applied this request, including
    /// its cleanup/reset obligations. Stale completion cannot acknowledge or
    /// clear reset intent belonging to a newer request.
    pub(crate) fn acknowledge(&self, request: ControlRequest, ack: ControlAck) -> bool {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            if state.current != request
                || !matches!(
                    (request.command, ack),
                    (
                        ControlCommand::Suspend,
                        ControlAck::Quiesced | ControlAck::CleanupFailed
                    ) | (ControlCommand::Resume { .. }, ControlAck::Running)
                )
            {
                return false;
            }
            state.acknowledgement = Some(ack);
            if ack == ControlAck::Running {
                state.reset_pending = false;
            }
            self.changed.signal(());
            true
        })
    }

    pub(crate) async fn wait(
        &self,
        request: ControlRequest,
        deadline: impl Future<Output = ()>,
    ) -> WaitOutcome {
        bounded(
            async {
                loop {
                    let result = self.state.lock(|cell| {
                        let state = cell.borrow();
                        if state.current != request {
                            Some(WaitOutcome::Superseded)
                        } else {
                            state.acknowledgement.map(WaitOutcome::Acknowledged)
                        }
                    });
                    if let Some(result) = result {
                        return result;
                    }
                    self.changed.wait().await;
                }
            },
            deadline,
        )
        .await
        .unwrap_or(WaitOutcome::TimedOut)
    }

    pub(crate) async fn wait_suspended(
        &self,
        request: ControlRequest,
        deadline: impl Future<Output = ()>,
    ) -> SuspendAck {
        match self.wait(request, deadline).await {
            WaitOutcome::Acknowledged(ControlAck::Quiesced) => SuspendAck::Quiesced,
            WaitOutcome::Acknowledged(ControlAck::CleanupFailed) => SuspendAck::CleanupFailed,
            WaitOutcome::Superseded => SuspendAck::Superseded,
            _ => SuspendAck::TimedOut,
        }
    }

    pub(crate) async fn suspend(&self, deadline: impl Future<Output = ()>) -> SuspendAck {
        let Some(request) = self.request_suspend() else {
            return SuspendAck::Exhausted;
        };
        self.wait_suspended(request, deadline).await
    }

    pub(crate) async fn resume(
        &self,
        reset_pipeline: bool,
        deadline: impl Future<Output = ()>,
    ) -> bool {
        let request = self.request_resume(reset_pipeline);
        self.wait(request, deadline).await == WaitOutcome::Acknowledged(ControlAck::Running)
    }

    /// Common actor loop for participants with no resume-side peripheral work.
    /// The caller has already completed acquisition and attempted cleanup.
    pub(crate) async fn hold_suspended(&self, mut request: ControlRequest, cleanly: bool) {
        loop {
            match request.command {
                ControlCommand::Suspend => {
                    self.acknowledge(
                        request,
                        if cleanly {
                            ControlAck::Quiesced
                        } else {
                            ControlAck::CleanupFailed
                        },
                    );
                }
                ControlCommand::Resume { .. } => {
                    self.acknowledge(request, ControlAck::Running);
                    return;
                }
            }
            request = self.receive().await;
        }
    }
}

pub(crate) const CLIENT_COUNT: usize = 5;

pub(crate) fn all_permit_shared_bus_access(acks: [SuspendAck; CLIENT_COUNT]) -> bool {
    acks.iter().all(|ack| ack.permits_shared_bus_access())
}

#[cfg(test)]
mod tests {
    use super::*;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;

    #[test]
    fn request_exhaustion_rejects_suspension_but_preserves_recovery() {
        let control = Control::<NoopRawMutex>::new();
        control
            .state
            .lock(|cell| cell.borrow_mut().current.id = u32::MAX - 2);
        let last_suspend = control.request_suspend().unwrap();
        assert_eq!(last_suspend.id(), u32::MAX - 1);
        let recovery = control.request_resume(true);
        assert_eq!(recovery.id(), u32::MAX);
        assert!(control.request_suspend().is_none());
        assert!(!control.acknowledge(last_suspend, ControlAck::Quiesced));
        assert_eq!(control.try_receive(), Some(recovery));
        assert!(control.acknowledge(recovery, ControlAck::Running));
        assert_eq!(
            control.request_resume(false).command,
            ControlCommand::Resume {
                reset_pipeline: false
            }
        );
        assert!(control.request_suspend().is_none());
    }
}
