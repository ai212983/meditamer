use super::{navigator::NavigationFrame, types::SurfaceInstanceToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleEvent {
    CandidateEntered,
    CandidateActivated,
    OriginQuiesced,
    ShellCommitted,
    CandidateEnabled,
    OriginDestroyed,
    RolledBack,
    CleanupBlocked,
}

pub trait SurfaceRuntime {
    type Instance;
    type EnterError;

    fn enter(
        &mut self,
        frame: NavigationFrame,
        token: SurfaceInstanceToken,
    ) -> Result<Self::Instance, Self::EnterError>;
    fn activate(&mut self, instance: &Self::Instance) -> bool;
    fn quiesce(&mut self, instance: &Self::Instance) -> bool;
    fn enable(&mut self, instance: &Self::Instance) -> bool;
    fn destroy(&mut self, instance: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>>;
    fn observe(&mut self, _event: LifecycleEvent) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollbackReason<EnterError, CommitError> {
    Entry(EnterError),
    Activation {
        origin_restored: bool,
    },
    Quiesce {
        origin_restored: bool,
    },
    CandidateEnable {
        origin_restored: bool,
    },
    Commit {
        error: CommitError,
        origin_restored: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostCommitFailure {
    OriginCleanup,
}

pub enum DestroyFailure<Instance> {
    Live(Instance),
    Audit,
}

pub enum TransitionResult<Instance, Outcome, EnterError, CommitError> {
    Committed {
        active: Instance,
        outcome: Outcome,
    },
    RolledBack {
        active: Instance,
        cleanup_blocked: Option<Instance>,
        cleanup_audit_failed: bool,
        reason: RollbackReason<EnterError, CommitError>,
    },
    FaultedAfterCommit {
        active: Instance,
        cleanup_blocked: Option<Instance>,
        cleanup_audit_failed: bool,
        outcome: Outcome,
        reason: PostCommitFailure,
    },
}

pub fn execute_transition<Runtime, Outcome, CommitError>(
    runtime: &mut Runtime,
    origin: Runtime::Instance,
    destination: NavigationFrame,
    destination_token: SurfaceInstanceToken,
    commit: impl FnOnce() -> Result<Outcome, CommitError>,
) -> TransitionResult<Runtime::Instance, Outcome, Runtime::EnterError, CommitError>
where
    Runtime: SurfaceRuntime,
{
    let candidate = match runtime.enter(destination, destination_token) {
        Ok(candidate) => candidate,
        Err(error) => {
            return TransitionResult::RolledBack {
                active: origin,
                cleanup_blocked: None,
                cleanup_audit_failed: false,
                reason: RollbackReason::Entry(error),
            };
        }
    };
    runtime.observe(LifecycleEvent::CandidateEntered);

    if !runtime.activate(&candidate) {
        let origin_restored = runtime.activate(&origin);
        let (cleanup_blocked, cleanup_audit_failed) = destroy_failure(runtime.destroy(candidate));
        runtime.observe(if cleanup_blocked.is_some() || cleanup_audit_failed {
            LifecycleEvent::CleanupBlocked
        } else {
            LifecycleEvent::RolledBack
        });
        return TransitionResult::RolledBack {
            active: origin,
            cleanup_blocked,
            cleanup_audit_failed,
            reason: RollbackReason::Activation { origin_restored },
        };
    }
    runtime.observe(LifecycleEvent::CandidateActivated);

    if !runtime.quiesce(&origin) {
        let origin_restored = runtime.activate(&origin) && runtime.enable(&origin);
        let (cleanup_blocked, cleanup_audit_failed) = destroy_failure(runtime.destroy(candidate));
        runtime.observe(if cleanup_blocked.is_some() || cleanup_audit_failed {
            LifecycleEvent::CleanupBlocked
        } else {
            LifecycleEvent::RolledBack
        });
        return TransitionResult::RolledBack {
            active: origin,
            cleanup_blocked,
            cleanup_audit_failed,
            reason: RollbackReason::Quiesce { origin_restored },
        };
    }
    runtime.observe(LifecycleEvent::OriginQuiesced);

    if !runtime.enable(&candidate) {
        let origin_restored = runtime.activate(&origin) && runtime.enable(&origin);
        let (cleanup_blocked, cleanup_audit_failed) = destroy_failure(runtime.destroy(candidate));
        runtime.observe(if cleanup_blocked.is_some() || cleanup_audit_failed {
            LifecycleEvent::CleanupBlocked
        } else {
            LifecycleEvent::RolledBack
        });
        return TransitionResult::RolledBack {
            active: origin,
            cleanup_blocked,
            cleanup_audit_failed,
            reason: RollbackReason::CandidateEnable { origin_restored },
        };
    }
    runtime.observe(LifecycleEvent::CandidateEnabled);

    let outcome = match commit() {
        Ok(outcome) => outcome,
        Err(error) => {
            let origin_restored = runtime.activate(&origin) && runtime.enable(&origin);
            let (cleanup_blocked, cleanup_audit_failed) =
                destroy_failure(runtime.destroy(candidate));
            runtime.observe(if cleanup_blocked.is_some() || cleanup_audit_failed {
                LifecycleEvent::CleanupBlocked
            } else {
                LifecycleEvent::RolledBack
            });
            return TransitionResult::RolledBack {
                active: origin,
                cleanup_blocked,
                cleanup_audit_failed,
                reason: RollbackReason::Commit {
                    error,
                    origin_restored,
                },
            };
        }
    };
    runtime.observe(LifecycleEvent::ShellCommitted);

    if let Err(failure) = runtime.destroy(origin) {
        let (cleanup_blocked, cleanup_audit_failed) = destroy_failure(Err(failure));
        runtime.observe(LifecycleEvent::CleanupBlocked);
        return TransitionResult::FaultedAfterCommit {
            active: candidate,
            cleanup_blocked,
            cleanup_audit_failed,
            outcome,
            reason: PostCommitFailure::OriginCleanup,
        };
    }
    runtime.observe(LifecycleEvent::OriginDestroyed);
    TransitionResult::Committed {
        active: candidate,
        outcome,
    }
}

fn destroy_failure<Instance>(
    result: Result<(), DestroyFailure<Instance>>,
) -> (Option<Instance>, bool) {
    match result {
        Ok(()) => (None, false),
        Err(DestroyFailure::Live(instance)) => (Some(instance), false),
        Err(DestroyFailure::Audit) => (None, true),
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use heapless::Vec;

    use super::super::types::{
        InstanceGeneration, ProviderGeneration, ProviderId, ProviderToken, SurfaceId, SurfaceRef,
        SurfaceRole,
    };
    use super::*;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct FakeInstance(u8);

    struct FakeRuntime {
        events: Vec<LifecycleEvent, 8>,
        enter_ok: bool,
        activation_ok: bool,
        restore_ok: bool,
        quiesce_ok: bool,
        enable_ok: bool,
        cleanup_ok: bool,
        cleanup_audit_failed: bool,
        live: u8,
        peak_live: u8,
    }

    impl FakeRuntime {
        fn healthy() -> Self {
            Self {
                events: Vec::new(),
                enter_ok: true,
                activation_ok: true,
                restore_ok: true,
                quiesce_ok: true,
                enable_ok: true,
                cleanup_ok: true,
                cleanup_audit_failed: false,
                live: 1,
                peak_live: 1,
            }
        }
    }

    impl SurfaceRuntime for FakeRuntime {
        type Instance = FakeInstance;
        type EnterError = &'static str;

        fn enter(
            &mut self,
            _frame: NavigationFrame,
            _token: SurfaceInstanceToken,
        ) -> Result<Self::Instance, Self::EnterError> {
            if !self.enter_ok {
                return Err("enter");
            }
            self.live += 1;
            self.peak_live = self.peak_live.max(self.live);
            Ok(FakeInstance(2))
        }

        fn activate(&mut self, instance: &Self::Instance) -> bool {
            if instance.0 == 1 {
                self.restore_ok
            } else {
                self.activation_ok
            }
        }

        fn quiesce(&mut self, _instance: &Self::Instance) -> bool {
            self.quiesce_ok
        }

        fn enable(&mut self, instance: &Self::Instance) -> bool {
            instance.0 == 1 || self.enable_ok
        }

        fn destroy(
            &mut self,
            instance: Self::Instance,
        ) -> Result<(), DestroyFailure<Self::Instance>> {
            if !self.cleanup_ok {
                return Err(DestroyFailure::Live(instance));
            }
            self.live -= 1;
            if self.cleanup_audit_failed {
                return Err(DestroyFailure::Audit);
            }
            Ok(())
        }

        fn observe(&mut self, event: LifecycleEvent) {
            self.events.push(event).unwrap();
        }
    }

    fn destination() -> (NavigationFrame, SurfaceInstanceToken) {
        let owner = ProviderToken::issued(ProviderId(1), ProviderGeneration(1));
        let surface = SurfaceRef {
            owner,
            id: SurfaceId(2),
        };
        (
            NavigationFrame {
                surface,
                role: SurfaceRole::Launcher,
            },
            SurfaceInstanceToken::issued(surface, InstanceGeneration(2)),
        )
    }

    #[test]
    fn successful_transition_has_two_live_only_during_handoff() {
        let mut runtime = FakeRuntime::healthy();
        let (destination, token) = destination();
        let result = execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
            Ok::<_, ()>("committed")
        });

        assert!(matches!(
            result,
            TransitionResult::Committed {
                active: FakeInstance(2),
                outcome: "committed"
            }
        ));
        assert_eq!(runtime.live, 1);
        assert_eq!(runtime.peak_live, 2);
        assert_eq!(
            runtime.events.as_slice(),
            &[
                LifecycleEvent::CandidateEntered,
                LifecycleEvent::CandidateActivated,
                LifecycleEvent::OriginQuiesced,
                LifecycleEvent::CandidateEnabled,
                LifecycleEvent::ShellCommitted,
                LifecycleEvent::OriginDestroyed,
            ]
        );
    }

    #[test]
    fn entry_activation_and_commit_failures_restore_origin_and_cleanup_candidate() {
        let (destination, token) = destination();
        for failure in 0..3 {
            let mut runtime = FakeRuntime::healthy();
            runtime.enter_ok = failure != 0;
            runtime.activation_ok = failure != 1;
            let result =
                execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
                    (failure != 2).then_some(()).ok_or("commit")
                });
            assert!(matches!(
                result,
                TransitionResult::RolledBack {
                    active: FakeInstance(1),
                    cleanup_blocked: None,
                    ..
                }
            ));
            assert_eq!(runtime.live, 1);
        }
    }

    #[test]
    fn cleanup_failure_is_explicit_and_blocks_reclamation_claim() {
        let mut runtime = FakeRuntime::healthy();
        runtime.cleanup_ok = false;
        let (destination, token) = destination();
        let result = execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
            Ok::<_, ()>(())
        });

        assert!(matches!(
            result,
            TransitionResult::FaultedAfterCommit {
                active: FakeInstance(2),
                cleanup_blocked: Some(FakeInstance(1)),
                reason: PostCommitFailure::OriginCleanup,
                ..
            }
        ));
        assert_eq!(runtime.live, 2);
        assert_eq!(runtime.events.last(), Some(&LifecycleEvent::CleanupBlocked));
    }

    #[test]
    fn transient_cleanup_failure_can_be_retried_without_losing_the_active_candidate() {
        let mut runtime = FakeRuntime::healthy();
        runtime.cleanup_ok = false;
        let (destination, token) = destination();
        let (active, blocked) =
            match execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
                Ok::<_, ()>(())
            }) {
                TransitionResult::FaultedAfterCommit {
                    active,
                    cleanup_blocked: Some(blocked),
                    ..
                } => (active, blocked),
                _ => panic!("leave failure must retain both owned instances"),
            };
        assert_eq!(active, FakeInstance(2));
        assert_eq!(runtime.live, 2);

        runtime.cleanup_ok = true;
        assert!(runtime.destroy(blocked).is_ok());
        assert_eq!(runtime.live, 1);
    }

    #[test]
    fn post_delete_audit_failure_never_returns_the_deleted_instance() {
        let mut runtime = FakeRuntime::healthy();
        runtime.cleanup_audit_failed = true;
        let (destination, token) = destination();
        let result = execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
            Ok::<_, ()>(())
        });

        assert!(matches!(
            result,
            TransitionResult::FaultedAfterCommit {
                active: FakeInstance(2),
                cleanup_blocked: None,
                cleanup_audit_failed: true,
                ..
            }
        ));
        assert_eq!(runtime.live, 1);
    }

    #[test]
    fn quiesce_and_candidate_enable_failures_restore_the_origin() {
        let (destination, token) = destination();
        let mut quiesce_failure = FakeRuntime::healthy();
        quiesce_failure.quiesce_ok = false;
        let rolled_back = execute_transition(
            &mut quiesce_failure,
            FakeInstance(1),
            destination,
            token,
            || Ok::<_, ()>(()),
        );
        assert!(matches!(
            rolled_back,
            TransitionResult::RolledBack {
                active: FakeInstance(1),
                cleanup_blocked: None,
                reason: RollbackReason::Quiesce {
                    origin_restored: true
                },
                ..
            }
        ));
        assert_eq!(quiesce_failure.live, 1);

        let mut enable_failure = FakeRuntime::healthy();
        enable_failure.enable_ok = false;
        let rolled_back = execute_transition(
            &mut enable_failure,
            FakeInstance(1),
            destination,
            token,
            || Ok::<_, ()>(()),
        );
        assert!(matches!(
            rolled_back,
            TransitionResult::RolledBack {
                active: FakeInstance(1),
                cleanup_blocked: None,
                reason: RollbackReason::CandidateEnable {
                    origin_restored: true
                },
                ..
            }
        ));
        assert_eq!(enable_failure.live, 1);
    }

    #[test]
    fn failed_origin_restore_is_explicit_and_allows_a_second_base_recovery_transition() {
        let (destination, token) = destination();
        let mut runtime = FakeRuntime::healthy();
        runtime.activation_ok = false;
        runtime.restore_ok = false;
        let origin =
            match execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
                Ok::<_, ()>(())
            }) {
                TransitionResult::RolledBack {
                    active,
                    cleanup_blocked: None,
                    cleanup_audit_failed: false,
                    reason:
                        RollbackReason::Activation {
                            origin_restored: false,
                        },
                } => active,
                _ => panic!("failed restoration must be explicit"),
            };
        assert_eq!(runtime.live, 1);

        runtime.activation_ok = true;
        runtime.restore_ok = true;
        let recovered =
            execute_transition(&mut runtime, origin, destination, token, || Ok::<_, ()>(()));
        assert!(matches!(
            recovered,
            TransitionResult::Committed {
                active: FakeInstance(2),
                ..
            }
        ));
        assert_eq!(runtime.live, 1);
    }

    #[test]
    fn blocked_candidate_cleanup_can_retry_before_base_recovery() {
        let (destination, token) = destination();
        let mut runtime = FakeRuntime::healthy();
        runtime.activation_ok = false;
        runtime.restore_ok = false;
        runtime.cleanup_ok = false;
        let (origin, blocked) =
            match execute_transition(&mut runtime, FakeInstance(1), destination, token, || {
                Ok::<_, ()>(())
            }) {
                TransitionResult::RolledBack {
                    active,
                    cleanup_blocked: Some(blocked),
                    reason:
                        RollbackReason::Activation {
                            origin_restored: false,
                        },
                    ..
                } => (active, blocked),
                _ => panic!("combined rollback failure must retain both instances"),
            };
        assert_eq!(runtime.live, 2);

        runtime.cleanup_ok = true;
        assert!(runtime.destroy(blocked).is_ok());
        assert_eq!(runtime.live, 1);
        runtime.activation_ok = true;
        runtime.restore_ok = true;
        let recovered =
            execute_transition(&mut runtime, origin, destination, token, || Ok::<_, ()>(()));
        assert!(matches!(recovered, TransitionResult::Committed { .. }));
        assert_eq!(runtime.live, 1);
    }

    #[test]
    fn repeated_transitions_return_to_one_live_instance() {
        let mut runtime = FakeRuntime::healthy();
        let (destination, mut token) = destination();
        let mut active = FakeInstance(1);
        for generation in 2..=101 {
            runtime.events.clear();
            token.generation = InstanceGeneration(generation);
            active = match execute_transition(&mut runtime, active, destination, token, || {
                Ok::<_, ()>(())
            }) {
                TransitionResult::Committed { active, .. } => active,
                _ => panic!("healthy fake transition must commit"),
            };
            assert_eq!(runtime.live, 1);
        }
        assert_eq!(runtime.peak_live, 2);
    }
}
