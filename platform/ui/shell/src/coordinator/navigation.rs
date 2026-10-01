//! Screen navigation dispatch and recovery when a transition or blocked
//! cleanup cannot leave the coordinator usable.

use heapless::Vec;

use super::super::lifecycle::{
    execute_transition, DestroyFailure, RollbackReason, SurfaceRuntime, TransitionResult,
};
use super::super::navigator::NavigationOutcome;
use super::super::types::{NavIntent, SurfaceInstanceToken};
use super::{
    destroy_composition_instances, stage_composition_entries, CompositionRuntime, DispatchRejected,
    NavigationDispatchOutcome, UiCoordinator,
};

/// Whether a rolled-back [`TransitionResult`] left the origin instance
/// restored and usable -- an entry failure never touched the origin at all,
/// so it always counts as restored; every other reason carries its own
/// `origin_restored` flag from [`execute_transition`]'s own bookkeeping.
fn origin_restored<EnterError, CommitError>(
    reason: &RollbackReason<EnterError, CommitError>,
) -> bool {
    match reason {
        RollbackReason::Entry(_) => true,
        RollbackReason::Activation { origin_restored }
        | RollbackReason::Quiesce { origin_restored }
        | RollbackReason::CandidateEnable { origin_restored }
        | RollbackReason::Commit {
            origin_restored, ..
        } => *origin_restored,
    }
}

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
    /// Restores the active screen after a rolled-back navigation and decides
    /// whether the rollback left the coordinator faulted -- shared logic
    /// [`Self::retry_blocked_cleanup`] also needs once its own blocked
    /// screen is confirmed gone. Matches `Backend::settle_rolled_back`.
    fn settle_rolled_back_screen<ScreenRt, OverlayRt>(
        &mut self,
        active: Screen,
        cleanup_blocked: Option<Screen>,
        cleanup_audit_failed: bool,
        origin_restored: bool,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        if origin_restored {
            self.active_screen = Some(active);
            self.cleanup_blocked_screen = cleanup_blocked;
            self.navigation_faulted = self.cleanup_blocked_screen.is_some() || cleanup_audit_failed;
        } else if cleanup_blocked.is_none() && !cleanup_audit_failed {
            let recovered = self.recovery_transition(active, screen_runtime, overlay_runtime);
            self.navigation_faulted = !recovered;
        } else {
            self.active_screen = Some(active);
            self.cleanup_blocked_screen = cleanup_blocked;
            self.navigation_faulted = true;
        }
    }

    /// Forces the ambient Home screen in, always through a full
    /// [`execute_transition`] against `origin` -- never
    /// [`Self::active_screen`], which the caller may itself be in the
    /// middle of recovering -- and always with a freshly minted destination
    /// instance (`ShellModel::prepare_recovery_home`, unlike
    /// [`Self::dispatch_navigation`]'s plain `NavIntent::Home`, never reuses
    /// the current active instance token even if the navigator already
    /// considers Home active: that token may be exactly the broken one this
    /// call exists to replace). Returns whether recovery committed. Matches
    /// `Backend::recover_ambient`.
    fn recovery_transition<ScreenRt, OverlayRt>(
        &mut self,
        origin: Screen,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) -> bool
    where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        let Ok(prepared) = self.shell.prepare_recovery_home() else {
            self.active_screen = Some(origin);
            return false;
        };
        let mut blocked: Vec<Overlay, OVERLAYS> = Vec::new();
        let Some(entered) = stage_composition_entries::<OverlayRt, OVERLAYS>(
            overlay_runtime,
            prepared.composition_delta().enter_live(),
            &mut blocked,
        ) else {
            self.absorb_blocked_overlays(blocked, false);
            self.active_screen = Some(origin);
            return false;
        };
        let mut departing_tokens: Vec<SurfaceInstanceToken, OVERLAYS> = Vec::new();
        for instance in prepared.composition_delta().leave_live() {
            let _ = departing_tokens.push(instance.token);
        }
        if !self.disable_live(&departing_tokens, overlay_runtime) {
            let audit_failed =
                destroy_composition_instances(overlay_runtime, entered, &mut blocked);
            self.absorb_blocked_overlays(blocked, audit_failed);
            self.active_screen = Some(origin);
            self.composition_faulted = true;
            return false;
        }
        let destination = prepared.destination();
        let destination_instance = prepared.destination_instance();
        match execute_transition(
            screen_runtime,
            origin,
            destination,
            destination_instance,
            || self.shell.commit_navigation(prepared),
        ) {
            TransitionResult::Committed { active, .. } => {
                self.active_screen = Some(active);
                self.cleanup_blocked_screen = None;
                self.complete_composition(entered, &departing_tokens, overlay_runtime);
                true
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason: _,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active_screen = Some(active);
                self.cleanup_blocked_screen = cleanup_blocked;
                let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                let audit_failed =
                    destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                self.absorb_blocked_overlays(blocked, audit_failed);
                // The caller sets `navigation_faulted` from this `false`
                // return already; an overlay this recovery attempt could
                // not actually restore is a distinct, sharper failure --
                // Home recovery itself left something shown but disabled --
                // so it also faults composition explicitly rather than
                // reporting a clean-looking failed recovery.
                self.composition_faulted |= !overlay_restored;
                false
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                ..
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.active_screen = Some(active);
                self.cleanup_blocked_screen = cleanup_blocked;
                self.complete_composition(entered, &departing_tokens, overlay_runtime);
                false
            }
        }
    }

    /// Dispatches a navigation event. Every navigation intent carries a
    /// bundled composition delta too (`ShellModel::prepare_intent` always
    /// plans a transient-overlay drop alongside it, which can itself
    /// promote a queued modal into the live slot) -- staged the same way
    /// [`Self::dispatch_composition`] stages one, before the screen swap
    /// itself runs through [`execute_transition`], matching
    /// `Backend::drain_shell_navigation` exactly.
    pub fn dispatch_navigation<ScreenRt, OverlayRt>(
        &mut self,
        intent: NavIntent,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) -> Result<NavigationDispatchOutcome, DispatchRejected>
    where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        if self.is_faulted() {
            return Err(DispatchRejected::Faulted);
        }
        let prepared = self
            .shell
            .prepare_intent(intent)
            .map_err(DispatchRejected::Navigation)?;

        let mut blocked: Vec<Overlay, OVERLAYS> = Vec::new();
        let Some(entered) = stage_composition_entries::<OverlayRt, OVERLAYS>(
            overlay_runtime,
            prepared.composition_delta().enter_live(),
            &mut blocked,
        ) else {
            self.absorb_blocked_overlays(blocked, false);
            return Ok(NavigationDispatchOutcome::OverlayEntryRejected);
        };

        let mut departing_tokens: Vec<SurfaceInstanceToken, OVERLAYS> = Vec::new();
        for instance in prepared.composition_delta().leave_live() {
            let _ = departing_tokens.push(instance.token);
        }

        if !self.disable_live(&departing_tokens, overlay_runtime) {
            let audit_failed =
                destroy_composition_instances(overlay_runtime, entered, &mut blocked);
            self.absorb_blocked_overlays(blocked, audit_failed);
            self.navigation_faulted = true;
            self.composition_faulted = true;
            return Ok(NavigationDispatchOutcome::OverlayRuntimeMisaligned);
        }

        let active_instance = self.shell.active_instance();
        if !prepared.requires_reentry(active_instance) {
            return Ok(match self.shell.commit_navigation(prepared) {
                Ok(outcome) => {
                    self.complete_composition(entered, &departing_tokens, overlay_runtime);
                    NavigationDispatchOutcome::Committed {
                        changed: outcome == NavigationOutcome::Changed,
                    }
                }
                Err(_) => {
                    let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                    let audit_failed =
                        destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                    self.absorb_blocked_overlays(blocked, audit_failed);
                    self.composition_faulted |= !overlay_restored;
                    NavigationDispatchOutcome::RolledBack
                }
            });
        }

        let Some(origin) = self.active_screen.take() else {
            let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
            let audit_failed =
                destroy_composition_instances(overlay_runtime, entered, &mut blocked);
            self.absorb_blocked_overlays(blocked, audit_failed);
            self.navigation_faulted = true;
            self.composition_faulted |= !overlay_restored;
            return Ok(NavigationDispatchOutcome::FaultedAfterCommit);
        };
        let destination = prepared.destination();
        let destination_instance = prepared.destination_instance();
        let result = execute_transition(
            screen_runtime,
            origin,
            destination,
            destination_instance,
            || self.shell.commit_navigation(prepared),
        );

        Ok(match result {
            TransitionResult::Committed { active, outcome } => {
                self.active_screen = Some(active);
                self.complete_composition(entered, &departing_tokens, overlay_runtime);
                NavigationDispatchOutcome::Committed {
                    changed: outcome == NavigationOutcome::Changed,
                }
            }
            TransitionResult::RolledBack {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                reason,
            } => {
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                let overlay_restored = self.enable_live(&departing_tokens, overlay_runtime);
                let audit_failed =
                    destroy_composition_instances(overlay_runtime, entered, &mut blocked);
                self.absorb_blocked_overlays(blocked, audit_failed);
                self.composition_faulted |= !overlay_restored;
                self.settle_rolled_back_screen(
                    active,
                    cleanup_blocked,
                    cleanup_audit_failed,
                    origin_restored(&reason),
                    screen_runtime,
                    overlay_runtime,
                );
                NavigationDispatchOutcome::RolledBack
            }
            TransitionResult::FaultedAfterCommit {
                active,
                cleanup_blocked,
                cleanup_audit_failed,
                outcome: _,
                reason: _,
            } => {
                self.active_screen = Some(active);
                self.cleanup_blocked_screen = cleanup_blocked;
                self.lifecycle_audit_faulted |= cleanup_audit_failed;
                self.navigation_faulted = true;
                self.complete_composition(entered, &departing_tokens, overlay_runtime);
                NavigationDispatchOutcome::FaultedAfterCommit
            }
        })
    }

    /// Retries a blocked screen cleanup, then any blocked overlay cleanups.
    /// Clears the corresponding fault once every retained instance is
    /// confirmed gone -- matches `Backend::retry_blocked_cleanup` +
    /// `retry_overlay_cleanup`, generalized past LVGL.
    pub fn retry_blocked_cleanup<ScreenRt, OverlayRt>(
        &mut self,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        // Only a *successful* screen-cleanup retry attempts forced recovery
        // below -- an audit failure or a still-live instance both leave
        // `navigation_faulted` exactly as `Backend::retry_blocked_cleanup`
        // did, and no screen cleanup pending this call (already `None`) is
        // `Backend::retry_overlay_cleanup`'s own success path instead, which
        // never recovers on its own.
        let screen_destroyed = if let Some(blocked) = self.cleanup_blocked_screen.take() {
            match screen_runtime.destroy(blocked) {
                Ok(()) => true,
                Err(DestroyFailure::Live(blocked)) => {
                    self.cleanup_blocked_screen = Some(blocked);
                    false
                }
                Err(DestroyFailure::Audit) => {
                    self.lifecycle_audit_faulted = true;
                    false
                }
            }
        } else {
            false
        };

        let mut remaining: Vec<Overlay, OVERLAYS> = Vec::new();
        while let Some(instance) = self.overlay_cleanup_blocked.pop() {
            match overlay_runtime.destroy(instance) {
                Ok(()) => {}
                Err(DestroyFailure::Live(instance)) => {
                    let _ = remaining.push(instance);
                }
                Err(DestroyFailure::Audit) => {
                    self.composition_faulted = true;
                }
            }
        }
        self.overlay_cleanup_blocked = remaining;

        if screen_destroyed {
            if self.composition_faulted || self.lifecycle_audit_faulted {
                self.navigation_faulted = true;
            } else if self.overlay_cleanup_blocked.is_empty() {
                self.navigation_faulted = false;
            } else if let Some(origin) = self.active_screen.take() {
                self.navigation_faulted =
                    !self.recovery_transition(origin, screen_runtime, overlay_runtime);
            } else {
                self.navigation_faulted = true;
            }
        } else if self.cleanup_blocked_screen.is_none()
            && self.overlay_cleanup_blocked.is_empty()
            && !self.composition_faulted
            && !self.lifecycle_audit_faulted
        {
            self.navigation_faulted = false;
        }
    }
}
