//! One UI owner and one provider actor: the newest suspend/resume intent wins.
//! Timing out or dropping a waiter never removes intent. Only cleanup for the
//! exact current request can authorize sleep; an old resume cannot wake it.

use core::{cell::RefCell, future::Future};
use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};
use observation::{
    field::FieldMask,
    ingress::RequestIngress,
    runtime::{AcquisitionDriver, ProviderLoop, StepOutcome, SuspendOutcome},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Suspend,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    id: u32,
    pub command: Command,
    reject_cleanup: bool,
}

impl Request {
    pub(crate) fn id(self) -> u32 {
        self.id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SuspendAck {
    Quiesced,
    CleanupFailed,
    TimedOut,
    Superseded,
    Exhausted,
}

impl SuspendAck {
    pub(crate) fn permits_sleep(self) -> bool {
        self == Self::Quiesced
    }
}

struct State {
    current: Request,
    acknowledgement: Option<SuspendOutcome>,
}

pub(crate) struct Control<M: RawMutex> {
    state: Mutex<M, RefCell<State>>,
    requests: Signal<M, Request>,
    changed: Signal<M, ()>,
}

impl<M: RawMutex> Control<M> {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                current: Request {
                    id: 0,
                    command: Command::Resume,
                    reject_cleanup: false,
                },
                acknowledgement: None,
            })),
            requests: Signal::new(),
            changed: Signal::new(),
        }
    }

    pub(crate) fn request_suspend(&self) -> Option<Request> {
        self.publish(Command::Suspend)
    }

    /// Recovery is always admitted, even when suspension identities run out.
    pub(crate) fn request_resume(&self) -> Request {
        self.publish(Command::Resume)
            .expect("resume is always admitted")
    }

    fn publish(&self, command: Command) -> Option<Request> {
        self.publish_with_rejection(command, false)
    }

    fn publish_with_rejection(&self, command: Command, reject_cleanup: bool) -> Option<Request> {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            let id = state.current.id.saturating_add(1);
            // The terminal identity is Running-only. No sleep authorization
            // can wrap or alias an earlier suspend request.
            if id == u32::MAX && command == Command::Suspend {
                return None;
            }
            let request = Request {
                id,
                command,
                reject_cleanup,
            };
            state.current = request;
            state.acknowledgement = None;
            self.requests.signal(request);
            self.changed.signal(());
            Some(request)
        })
    }

    pub(crate) fn is_current(&self, request: Request) -> bool {
        self.state.lock(|cell| cell.borrow().current == request)
    }

    pub(crate) fn snapshot(&self) -> (Request, Option<SuspendOutcome>) {
        self.state.lock(|cell| {
            let state = cell.borrow();
            (state.current, state.acknowledgement)
        })
    }

    pub(crate) fn try_receive(&self) -> Option<Request> {
        while let Some(request) = self.requests.try_take() {
            if self.is_current(request) {
                return Some(request);
            }
        }
        None
    }

    pub(crate) async fn receive(&self) -> Request {
        loop {
            let request = self.requests.wait().await;
            if self.is_current(request) {
                return request;
            }
        }
    }

    pub(crate) fn acknowledge(&self, request: Request, outcome: SuspendOutcome) -> bool {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            if state.current != request || request.command != Command::Suspend {
                return false;
            }
            state.acknowledgement = Some(outcome);
            self.changed.signal(());
            true
        })
    }

    pub(crate) async fn wait_suspended(
        &self,
        request: Request,
        deadline: impl Future<Output = ()>,
    ) -> SuspendAck {
        let wait = async {
            loop {
                let result = self.state.lock(|cell| {
                    let state = cell.borrow();
                    if state.current != request {
                        return Some(SuspendAck::Superseded);
                    }
                    state.acknowledgement.map(|outcome| match outcome {
                        SuspendOutcome::Quiesced => SuspendAck::Quiesced,
                        SuspendOutcome::CleanupFailed => SuspendAck::CleanupFailed,
                    })
                });
                if let Some(result) = result {
                    return result;
                }
                self.changed.wait().await;
            }
        };
        match select(wait, deadline).await {
            Either::First(outcome) => outcome,
            Either::Second(()) => SuspendAck::TimedOut,
        }
    }

    /// Close request admission synchronously, even before the returned waiter
    /// is polled. Dropping the waiter leaves the published intent in place.
    /// `reject_cleanup` injects a synthetic acknowledgement failure for this
    /// request only, after real cleanup. It does not simulate a physical fault.
    pub(crate) fn suspend<'a, F: FieldMask, const N: usize>(
        &'a self,
        ingress: &RequestIngress<M, F, N>,
        deadline: impl Future<Output = ()> + 'a,
        reject_cleanup: bool,
    ) -> impl Future<Output = SuspendAck> + 'a {
        ingress.close();
        let request = if reject_cleanup {
            self.publish_with_rejection(Command::Suspend, true)
        } else {
            self.request_suspend()
        };
        async move {
            match request {
                Some(request) => self.wait_suspended(request, deadline).await,
                None => SuspendAck::Exhausted,
            }
        }
    }

    /// Production actor: called between complete acquisitions, before another
    /// step may start. Each new suspend retries cleanup, even after a failure.
    /// Stay suspended until the latest resume is consumed; stale completion
    /// cannot acknowledge a newer request or displace its retained intent.
    pub(crate) async fn handle<
        F: FieldMask,
        S,
        D: AcquisitionDriver<F, S>,
        const N: usize,
        const FIELDS: usize,
    >(
        &self,
        provider: &mut ProviderLoop<F, S, D, N, FIELDS>,
        ingress: &RequestIngress<M, F, N>,
        label: &str,
        mut request: Request,
        now: impl Fn() -> Instant,
    ) {
        loop {
            if self.is_current(request) {
                match request.command {
                    Command::Suspend => {
                        ingress.close();
                        let pending = provider.pending_observe_now();
                        let mut outcome = provider.suspend(now()).await;
                        if request.reject_cleanup && outcome == SuspendOutcome::Quiesced {
                            outcome = SuspendOutcome::CleanupFailed;
                            #[cfg(target_os = "none")]
                            console::println!(
                                "OBSERVATION_CLEANUP_INJECTED provider={} id={}",
                                label,
                                request.id()
                            );
                        }
                        ingress.complete(pending);
                        let accepted = self.acknowledge(request, outcome);
                        #[cfg(target_os = "none")]
                        console::println!(
                            "OBSERVATION_SUSPEND_ACK provider={} id={} outcome={:?} accepted={}",
                            label,
                            request.id(),
                            outcome,
                            accepted
                        );
                        #[cfg(not(target_os = "none"))]
                        let _ = (accepted, label);
                    }
                    Command::Resume => {
                        provider.resume();
                        if !ingress.open() {
                            #[cfg(target_os = "none")]
                            console::println!(
                                "OBSERVATION_REQUEST provider={} rejected=GenerationExhausted",
                                label
                            );
                        }
                        return;
                    }
                }
            }
            request = self.receive().await;
        }
    }
    /// Counter exhaustion is terminal for acquisition/admission, but CPU sleep
    /// must still be able to request and acknowledge peripheral cleanup.
    pub(crate) async fn serve_stopped<
        F: FieldMask,
        S,
        D: AcquisitionDriver<F, S>,
        const N: usize,
        const FIELDS: usize,
    >(
        &self,
        provider: &mut ProviderLoop<F, S, D, N, FIELDS>,
        ingress: &RequestIngress<M, F, N>,
        label: &str,
        now: impl Fn() -> Instant,
    ) -> ! {
        ingress.close();
        loop {
            let request = self.receive().await;
            self.handle(provider, ingress, label, request, &now).await;
            // A current resume ends normal control handling. Reclose in this
            // same poll, before any caller can admit another request.
            ingress.close();
        }
    }
}

/// Bounded, synchronous admission shared by both production provider actors.
/// Call after checking control; a completed step can satisfy arrivals admitted
/// during its conversion without triggering another measurement.
pub(crate) fn drain_requests<
    M: RawMutex,
    F: FieldMask,
    S,
    D: AcquisitionDriver<F, S>,
    const N: usize,
    const FIELDS: usize,
>(
    provider: &mut ProviderLoop<F, S, D, N, FIELDS>,
    ingress: &RequestIngress<M, F, N>,
    outcome: Option<StepOutcome<F>>,
    now: impl Fn() -> Instant,
    label: &str,
) {
    for _ in 0..N {
        let Ok(request) = ingress.try_receive() else {
            break;
        };
        let before = provider.pending_observe_now();
        let admitted = match outcome {
            Some(outcome) => provider.admit_observe_now_after_step(request, outcome, now()),
            None => provider.admit_observe_now_at(request, now()),
        };
        ingress.complete(before + 1 - provider.pending_observe_now());
        #[cfg(target_os = "none")]
        if let Err(reason) = admitted {
            console::println!(
                "OBSERVATION_REQUEST provider={} rejected={:?} owner={} generation={}",
                label,
                reason,
                request.owner.0,
                request.owner_generation.0
            );
        }
        #[cfg(not(target_os = "none"))]
        let _ = (admitted, label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;

    #[test]
    fn exhaustion_rejects_sleep_but_retains_recovery() {
        let control = Control::<NoopRawMutex>::new();
        control
            .state
            .lock(|cell| cell.borrow_mut().current.id = u32::MAX - 2);
        let last = control.request_suspend().unwrap();
        assert_eq!(last.id(), u32::MAX - 1);
        let resume = control.request_resume();
        assert_eq!(resume.id(), u32::MAX);
        assert!(control.request_suspend().is_none());
        assert!(!control.acknowledge(last, SuspendOutcome::Quiesced));
        assert_eq!(control.try_receive(), Some(resume));
        assert_eq!(control.request_resume().command, Command::Resume);
        assert!(control.request_suspend().is_none());
    }
}
