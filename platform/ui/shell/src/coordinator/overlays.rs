//! Overlay admission and identity-specific removal.

use heapless::Vec;

use super::super::composition::CompositionPlanResult;
use super::super::types::{OverlayAdmission, OwnedCompositionIntent, SurfaceInstanceToken};
use super::{
    destroy_composition_instances, stage_composition_entries, CompositionDispatchOutcome,
    CompositionRollbackReason, CompositionRuntime, DispatchRejected, UiCoordinator,
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
    /// Runs a prepared composition change (admission or removal) that needs
    /// at least one target effect -- an active admission or a live removal.
    /// Queued admissions/removals need no effect and are committed directly
    /// by [`Self::dispatch_composition`] instead.
    ///
    /// Order matches `Backend::enter_overlay`/`remove_live_overlay`,
    /// proven on real hardware (E-0002/E-0003): stage every entering
    /// overlay's `enter`+`enable` first, then stage every leaving overlay's
    /// `disable`+`hide`, then commit to `ShellModel`, then -- only after a
    /// successful commit, via [`Self::complete_composition`] -- destroy the
    /// departed overlays and `show` the entered ones. A failure at any
    /// staging step tears back down whatever this attempt already staged
    /// and leaves the shell untouched.
    fn commit_composition_transaction<Runtime>(
        &mut self,
        prepared: super::super::model::PreparedComposition<OVERLAYS, MODALS>,
        runtime: &mut Runtime,
    ) -> CompositionDispatchOutcome
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        let mut blocked: Vec<Overlay, OVERLAYS> = Vec::new();
        let Some(candidates) = stage_composition_entries::<Runtime, OVERLAYS>(
            runtime,
            prepared.delta().enter_live(),
            &mut blocked,
        ) else {
            self.absorb_blocked_overlays(blocked, false);
            return CompositionDispatchOutcome::RolledBack(CompositionRollbackReason::Entry);
        };

        let mut departing_tokens: Vec<SurfaceInstanceToken, OVERLAYS> = Vec::new();
        for instance in prepared.delta().leave_live() {
            let _ = departing_tokens.push(instance.token);
        }

        if !self.disable_live(&departing_tokens, runtime) {
            let audit_failed = destroy_composition_instances(runtime, candidates, &mut blocked);
            self.absorb_blocked_overlays(blocked, audit_failed);
            return CompositionDispatchOutcome::RolledBack(CompositionRollbackReason::Departure);
        }

        match self.shell.commit_composition(prepared) {
            Ok(committed) => {
                self.complete_composition(candidates, &departing_tokens, runtime);
                match committed {
                    CompositionPlanResult::Admission(admission) => {
                        CompositionDispatchOutcome::Admitted(admission)
                    }
                    CompositionPlanResult::Removal(_) => CompositionDispatchOutcome::Removed,
                    CompositionPlanResult::Cleanup(_) => {
                        unreachable!("composition dispatch never produces a bare cleanup result")
                    }
                }
            }
            Err(_) => {
                let origin_restored = self.enable_live(&departing_tokens, runtime);
                let audit_failed = destroy_composition_instances(runtime, candidates, &mut blocked);
                self.absorb_blocked_overlays(blocked, audit_failed);
                // A departing overlay this rollback could not actually
                // re-enable is left shown but non-interactive -- fault
                // rather than report a clean rollback, the same way a
                // failed screen-origin restore does in `execute_transition`.
                self.composition_faulted |= !origin_restored;
                self.navigation_faulted |= !origin_restored;
                CompositionDispatchOutcome::RolledBack(CompositionRollbackReason::Departure)
            }
        }
    }

    /// Dispatches a composition (overlay) event: request an overlay or
    /// dismiss the active modal.
    pub fn dispatch_composition<Runtime>(
        &mut self,
        owned: OwnedCompositionIntent,
        runtime: &mut Runtime,
    ) -> Result<CompositionDispatchOutcome, DispatchRejected>
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        if self.is_faulted() {
            return Err(DispatchRejected::Faulted);
        }
        let prepared = self
            .shell
            .prepare_composition_intent(owned)
            .map_err(DispatchRejected::Composition)?;

        match prepared.result() {
            CompositionPlanResult::Admission(OverlayAdmission::Queued(instance)) => {
                match self.shell.commit_composition(prepared) {
                    Ok(CompositionPlanResult::Admission(committed))
                        if committed == OverlayAdmission::Queued(instance) =>
                    {
                        Ok(CompositionDispatchOutcome::Admitted(committed))
                    }
                    _ => Ok(CompositionDispatchOutcome::RolledBack(
                        CompositionRollbackReason::Departure,
                    )),
                }
            }
            CompositionPlanResult::Removal(dismissal) if !dismissal.removed_was_live => {
                match self.shell.commit_composition(prepared) {
                    Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {
                        Ok(CompositionDispatchOutcome::Removed)
                    }
                    _ => Ok(CompositionDispatchOutcome::RolledBack(
                        CompositionRollbackReason::Departure,
                    )),
                }
            }
            CompositionPlanResult::Admission(OverlayAdmission::Active(_))
            | CompositionPlanResult::Removal(_) => {
                Ok(self.commit_composition_transaction(prepared, runtime))
            }
            CompositionPlanResult::Cleanup(_) => {
                unreachable!("composition intents never produce a bare cleanup result")
            }
        }
    }

    /// Dispatches a removal for a specific overlay instance by token,
    /// regardless of whether it is the active modal -- unlike
    /// [`Self::dispatch_composition`]'s `CompositionIntent::DismissActiveModal`,
    /// which only ever targets the current modal, this is for a sticky,
    /// non-modal overlay a product removes directly by identity (Meditamer's
    /// refresh-control toggle-off, say). Matches `Backend::remove_settings_overlay`.
    pub fn dispatch_overlay_removal<Runtime>(
        &mut self,
        token: SurfaceInstanceToken,
        runtime: &mut Runtime,
    ) -> Result<CompositionDispatchOutcome, DispatchRejected>
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        if self.is_faulted() {
            return Err(DispatchRejected::Faulted);
        }
        let prepared = self
            .shell
            .prepare_overlay_removal(token)
            .map_err(DispatchRejected::Composition)?;
        let CompositionPlanResult::Removal(dismissal) = prepared.result() else {
            unreachable!("overlay removal plans always return a removal result")
        };
        if !dismissal.removed_was_live {
            return match self.shell.commit_overlay_removal(prepared) {
                Ok(CompositionPlanResult::Removal(committed)) if committed == dismissal => {
                    Ok(CompositionDispatchOutcome::Removed)
                }
                _ => Ok(CompositionDispatchOutcome::RolledBack(
                    CompositionRollbackReason::Departure,
                )),
            };
        }
        Ok(self.commit_composition_transaction(prepared, runtime))
    }
}
