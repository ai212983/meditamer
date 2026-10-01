//! Transactional provider-detach dispatch.
//!
//! Unlike navigation and composition, `ShellModel::commit_provider_detach`
//! commits its bundled navigation fallback itself rather than taking a
//! caller-supplied commit closure, so [`UiCoordinator::dispatch_provider_removal`]
//! can't reuse [`crate::lifecycle::execute_transition`] directly. The fallback
//! still follows its ordering: enter and activate the candidate, quiesce the
//! origin, enable the candidate, then commit. Any pre-commit failure restores
//! the origin before the candidate is destroyed.

use heapless::Vec;

use super::super::lifecycle::{DestroyFailure, LifecycleEvent, SurfaceRuntime};
use super::super::model::{
    PendingProviderRemoval, ProviderPurge, ProviderRemovalError, ProviderRuntimeAudit,
};
use super::super::types::{ProviderToken, SurfaceInstanceToken};
use super::{
    destroy_composition_instances, stage_composition_entries, CompositionRuntime, DispatchRejected,
    ProviderRemovalDispatchOutcome, UiCoordinator,
};

impl<
        Screen,
        Overlay,
        const PROVIDERS: usize,
        const SURFACES: usize,
        const NAVIGATION: usize,
        const OVERLAYS: usize,
        const MODALS: usize,
        const INTENTS: usize,
    > UiCoordinator<Screen, Overlay, PROVIDERS, SURFACES, NAVIGATION, OVERLAYS, MODALS, INTENTS>
{
    /// Dispatches a provider-detach event: removes a provider's shell and
    /// runtime references (its registry record survives until
    /// [`Self::finalize_provider_removal`]).
    pub fn dispatch_provider_removal<ScreenRt, OverlayRt>(
        &mut self,
        owner: ProviderToken,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) -> Result<ProviderRemovalDispatchOutcome, DispatchRejected>
    where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        if self.is_faulted() {
            return Err(DispatchRejected::Faulted);
        }
        let plan = self
            .shell
            .prepare_provider_removal(owner)
            .map_err(DispatchRejected::ProviderRemoval)?;

        let mut blocked: Vec<Overlay, OVERLAYS> = Vec::new();
        let Some(entered) = stage_composition_entries::<OverlayRt, OVERLAYS>(
            overlay_runtime,
            plan.composition_delta().enter_live(),
            &mut blocked,
        ) else {
            self.absorb_blocked_overlays(blocked, false);
            return Ok(ProviderRemovalDispatchOutcome::Rejected);
        };
        let mut departing_tokens: Vec<SurfaceInstanceToken, OVERLAYS> = Vec::new();
        for instance in plan.composition_delta().leave_live() {
            let _ = departing_tokens.push(instance.token);
        }
        if !self.disable_live(&departing_tokens, overlay_runtime) {
            let audit_failed =
                destroy_composition_instances(overlay_runtime, entered, &mut blocked);
            self.absorb_blocked_overlays(blocked, audit_failed);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            return Ok(ProviderRemovalDispatchOutcome::OverlayRuntimeMisaligned);
        }

        let fallback = plan.fallback_transition();
        let mut candidate_screen: Option<Screen> = None;
        let mut origin_screen: Option<Screen> = None;
        if let Some(transition) = fallback {
            let Some(origin) = self.active_screen.take() else {
                let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                let audit_failed =
                    destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                self.absorb_blocked_overlays(blocked, audit_failed);
                self.navigation_faulted = true;
                self.composition_faulted |= !overlay_restored;
                return Ok(ProviderRemovalDispatchOutcome::Rejected);
            };
            origin_screen = Some(origin);
            let origin = origin_screen.as_ref().expect("origin was just stored");
            let entry =
                screen_runtime.enter(transition.destination, transition.destination_instance);
            let candidate = match entry {
                Ok(candidate) => {
                    screen_runtime.observe(LifecycleEvent::CandidateEntered);
                    if !screen_runtime.activate(&candidate) {
                        let origin_restored = screen_runtime.activate(origin);
                        self.destroy_orphaned_candidate(screen_runtime, candidate);
                        self.active_screen = origin_screen.take();
                        self.navigation_faulted |= !origin_restored;
                        screen_runtime.observe(LifecycleEvent::RolledBack);
                        let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                        let audit_failed =
                            destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                        self.absorb_blocked_overlays(blocked, audit_failed);
                        self.composition_faulted |= !overlay_restored;
                        return Ok(ProviderRemovalDispatchOutcome::Rejected);
                    }
                    screen_runtime.observe(LifecycleEvent::CandidateActivated);
                    if !screen_runtime.quiesce(origin) {
                        let origin_restored =
                            screen_runtime.activate(origin) && screen_runtime.enable(origin);
                        self.destroy_orphaned_candidate(screen_runtime, candidate);
                        self.active_screen = origin_screen.take();
                        self.navigation_faulted |= !origin_restored;
                        screen_runtime.observe(LifecycleEvent::RolledBack);
                        let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                        let audit_failed =
                            destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                        self.absorb_blocked_overlays(blocked, audit_failed);
                        self.composition_faulted |= !overlay_restored;
                        return Ok(ProviderRemovalDispatchOutcome::Rejected);
                    }
                    screen_runtime.observe(LifecycleEvent::OriginQuiesced);
                    if !screen_runtime.enable(&candidate) {
                        let origin_restored =
                            screen_runtime.activate(origin) && screen_runtime.enable(origin);
                        self.destroy_orphaned_candidate(screen_runtime, candidate);
                        self.active_screen = origin_screen.take();
                        self.navigation_faulted |= !origin_restored;
                        screen_runtime.observe(LifecycleEvent::RolledBack);
                        let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                        let audit_failed =
                            destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                        self.absorb_blocked_overlays(blocked, audit_failed);
                        self.composition_faulted |= !overlay_restored;
                        return Ok(ProviderRemovalDispatchOutcome::Rejected);
                    }
                    screen_runtime.observe(LifecycleEvent::CandidateEnabled);
                    candidate
                }
                Err(_) => {
                    self.active_screen = origin_screen.take();
                    let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                    let audit_failed =
                        destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                    self.absorb_blocked_overlays(blocked, audit_failed);
                    self.composition_faulted |= !overlay_restored;
                    return Ok(ProviderRemovalDispatchOutcome::Rejected);
                }
            };
            candidate_screen = Some(candidate);
        }

        match self.shell.commit_provider_detach(plan) {
            Ok(pending) => {
                screen_runtime.observe(LifecycleEvent::ShellCommitted);
                self.complete_composition(entered, &departing_tokens, overlay_runtime);
                if let Some(candidate) = candidate_screen {
                    if let Some(origin) = origin_screen.take() {
                        match screen_runtime.destroy(origin) {
                            Ok(()) => {}
                            Err(DestroyFailure::Live(origin)) => {
                                self.cleanup_blocked_screen = Some(origin);
                                self.navigation_faulted = true;
                            }
                            Err(DestroyFailure::Audit) => {
                                self.lifecycle_audit_faulted = true;
                                self.navigation_faulted = true;
                            }
                        }
                        screen_runtime.observe(LifecycleEvent::OriginDestroyed);
                    }
                    self.active_screen = Some(candidate);
                }
                Ok(ProviderRemovalDispatchOutcome::Detached(pending))
            }
            Err(_) => {
                if let Some(candidate) = candidate_screen {
                    // The origin was quiesced before candidate enable; restore
                    // it before abandoning the candidate and shell transaction.
                    if let Some(origin) = origin_screen.take() {
                        let origin_restored =
                            screen_runtime.activate(&origin) && screen_runtime.enable(&origin);
                        self.destroy_orphaned_candidate(screen_runtime, candidate);
                        self.active_screen = Some(origin);
                        self.navigation_faulted |= !origin_restored;
                    } else {
                        self.destroy_orphaned_candidate(screen_runtime, candidate);
                        self.navigation_faulted = true;
                    }
                }
                let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                let audit_failed =
                    destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                self.absorb_blocked_overlays(blocked, audit_failed);
                self.composition_faulted |= !overlay_restored;
                Ok(ProviderRemovalDispatchOutcome::Rejected)
            }
        }
    }

    /// Proves the runtime holds no more references to `pending`'s provider
    /// (its callback routes, timers, or product-local bookkeeping) before
    /// releasing the registry record -- pure passthrough to `ShellModel`.
    pub fn finalize_provider_removal(
        &mut self,
        pending: &PendingProviderRemoval,
        audit: ProviderRuntimeAudit,
    ) -> Result<ProviderPurge, ProviderRemovalError> {
        self.shell.finalize_provider_removal(pending, audit)
    }
}
