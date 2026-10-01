//! Renderer-independent UI coordination.
//!
//! `UiCoordinator` prepares a candidate while the current composition stays
//! valid, commits shell and visible state together, and otherwise rolls back.
//! The same transaction rules apply to screens and overlays.
//!
//! Each product supplies two target adapters -- one [`SurfaceRuntime`] for
//! screens (reused unchanged from [`crate::lifecycle`]) and one
//! [`CompositionRuntime`] for overlays, matching its shape -- so the
//! coordinator itself never names a concrete renderer.
//!
//! This module owns shared contract types and composition primitives.
//! [`navigation`], [`overlays`], and [`provider_removal`] own their distinct
//! dispatch paths.

mod navigation;
mod overlays;
mod provider_removal;
#[cfg(all(test, not(target_os = "none")))]
mod tests;

use heapless::Vec;

use super::intent_queue::IntentQueueError;
use super::lifecycle::{DestroyFailure, SurfaceRuntime};
use super::model::{
    CompositionError, PendingProviderRemoval, ProviderRegistrationError, ProviderRemovalError,
    ShellInitError, ShellModel, ShellNavigationError,
};
use super::types::{
    NavIntent, OverlayAdmission, OverlayInstance, OwnedCompositionIntent, OwnedNavIntent,
    ProviderId, ProviderToken, RefreshHint, SurfaceId, SurfaceInstanceToken, SurfaceSpec,
};

/// Target adapter for overlays: the same enter/activate/quiesce/enable/destroy
/// shape [`SurfaceRuntime`] uses for screens, renamed for its role and kept as
/// a separate trait so one concrete type can implement both with different
/// `Instance`s. `activate`/`quiesce` are named `show`/`hide` here because an
/// overlay's on-screen state, unlike a screen's, is independent of whether it
/// currently accepts input (`enable`/`disable`).
pub trait CompositionRuntime {
    type Instance;
    type EnterError;

    fn enter(&mut self, instance: OverlayInstance) -> Result<Self::Instance, Self::EnterError>;
    /// The [`OverlayInstance`] a live handle was entered with, for the
    /// coordinator's own by-token bookkeeping.
    fn instance(&self, live: &Self::Instance) -> OverlayInstance;
    fn show(&mut self, live: &Self::Instance);
    fn hide(&mut self, live: &Self::Instance);
    fn enable(&mut self, live: &Self::Instance) -> bool;
    fn disable(&mut self, live: &Self::Instance) -> bool;
    fn destroy(&mut self, live: Self::Instance) -> Result<(), DestroyFailure<Self::Instance>>;
    /// Grants or revokes exclusive input capture for an active modal. Called
    /// once per modal entry/exit, not per overlay -- there is at most one
    /// live modal at a time (`ShellModel`'s composition rules enforce that).
    fn set_exclusive_capture(&mut self, _enabled: bool) {}
}

/// A shared navigation/composition action, or a product-specific payload the
/// coordinator does not interpret itself -- the product's own policy
/// translates `Product(P)` into `Navigate`/`Compose` events (or handles it
/// entirely on its own) before or after calling [`UiCoordinator::dispatch`].
/// `Compose` carries its source's identity (`OwnedCompositionIntent`), not a
/// bare `CompositionIntent`, because `ShellModel` checks it came from the
/// surface currently entitled to ask -- the active instance for a request,
/// the active modal for a dismissal; `Navigate` needs no such check
/// (`ShellModel::prepare_intent` takes a bare `NavIntent` for the same
/// reason: only *queued* navigation, drained later, is source-checked).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiEvent<P> {
    Navigate(NavIntent),
    Compose(OwnedCompositionIntent),
    Product(P),
}

/// [`UiCoordinator::dispatch`]'s result: which kind of event ran, and its
/// outcome in that kind's own terms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiDispatchResult {
    Navigation(Result<NavigationDispatchOutcome, DispatchRejected>),
    Composition(Result<CompositionDispatchOutcome, DispatchRejected>),
    ProductIgnored,
}

/// Why a dispatched event produced no transaction at all -- rejected before
/// any target effect ran, distinct from a transaction that ran effects and
/// then rolled back (see [`NavigationDispatchOutcome`]/[`CompositionDispatchOutcome`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchRejected {
    Navigation(ShellNavigationError),
    Composition(CompositionError),
    ProviderRemoval(ProviderRemovalError),
    /// The coordinator is latched after an unrecovered fault; see
    /// [`UiCoordinator::is_faulted`].
    Faulted,
}

/// A provider-detach event's outcome. Detaching only removes shell/runtime
/// *references*; the provider's registry record -- and its ability to be
/// re-registered -- survives until [`UiCoordinator::finalize_provider_removal`]
/// proves no reference remains, matching `ShellModel::commit_provider_detach`'s
/// own contract. Carries `PendingProviderRemoval` (not `Clone`/`Copy` --
/// `finalize_provider_removal` takes it by reference, so this outcome isn't
/// either).
pub enum ProviderRemovalDispatchOutcome {
    Detached(PendingProviderRemoval),
    /// Staging (fallback screen entry, or the bundled overlay delta) failed
    /// before anything committed; a soft rejection, not a fault.
    Rejected,
    /// A departing overlay's `disable` failed after entries were already
    /// staged. Matches [`NavigationDispatchOutcome::OverlayRuntimeMisaligned`].
    OverlayRuntimeMisaligned,
}

/// [`UiEvent::Product`]'s result: the coordinator does not interpret product
/// payloads, so dispatching one is always a no-op from its perspective.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductEventIgnored;

/// Why [`UiCoordinator::bootstrap_screen`] failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapError<EnterError> {
    Enter(EnterError),
    Activation,
    Enable,
}

/// A navigation event's outcome -- the same shapes as
/// [`crate::lifecycle::TransitionResult`], with the instances collapsed
/// since the coordinator owns them internally (see
/// [`UiCoordinator::active_screen_ready`]/[`UiCoordinator::is_faulted`] to
/// inspect what happened), plus the two outcomes specific to the overlay
/// delta every navigation carries alongside its screen swap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationDispatchOutcome {
    Committed {
        changed: bool,
    },
    RolledBack,
    FaultedAfterCommit,
    /// A promoted overlay's `enter`/`enable` failed before anything
    /// committed -- a soft rejection, not a fault: nothing changed.
    OverlayEntryRejected,
    /// A departing overlay's `disable` failed after entries were already
    /// staged. The coordinator is now faulted (see [`UiCoordinator::is_faulted`])
    /// and needs [`UiCoordinator::retry_blocked_cleanup`] before further
    /// dispatch -- matches `Backend`'s `overlay_runtime_misaligned` fault.
    OverlayRuntimeMisaligned,
}

/// An overlay admission or removal committed, or was rolled back before any
/// shell state changed. Composition staging failures never partially commit
/// -- either every entering overlay is live and every leaving one is gone,
/// or the composition is exactly as it was (matching
/// [`crate::composition::CompositionReferences`]'s own all-or-nothing
/// `admit`/`remove`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionDispatchOutcome {
    Admitted(OverlayAdmission),
    Removed,
    RolledBack(CompositionRollbackReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionRollbackReason {
    /// An entering overlay's `enter`/`enable` failed before the shell
    /// committed anything.
    Entry,
    /// A leaving overlay's `disable` failed after entries were already
    /// staged; the staged entries were torn back down.
    Departure,
}

/// Destroys one instance. Returns `true` if `destroy` reported an audit
/// failure (the object was actually gone but the runtime's own bookkeeping
/// -- callback routes, registries -- could not confirm it); a still-live
/// instance is pushed to `blocked` for the caller to retry later, exactly
/// like `DestroyFailure::Live` one level up in
/// `crate::lifecycle::execute_transition`.
fn destroy_one_composition_instance<Runtime, const OVERLAYS: usize>(
    runtime: &mut Runtime,
    instance: Runtime::Instance,
    blocked: &mut Vec<Runtime::Instance, OVERLAYS>,
) -> bool
where
    Runtime: CompositionRuntime,
{
    match runtime.destroy(instance) {
        Ok(()) => false,
        Err(DestroyFailure::Live(instance)) => {
            let _ = blocked.push(instance);
            false
        }
        Err(DestroyFailure::Audit) => true,
    }
}

fn destroy_composition_instances<Runtime, const OVERLAYS: usize>(
    runtime: &mut Runtime,
    mut instances: Vec<Runtime::Instance, OVERLAYS>,
    blocked: &mut Vec<Runtime::Instance, OVERLAYS>,
) -> bool
where
    Runtime: CompositionRuntime,
{
    let mut audit_failed = false;
    while let Some(instance) = instances.pop() {
        audit_failed |= destroy_one_composition_instance(runtime, instance, blocked);
    }
    audit_failed
}

/// Runs `enter` then `enable` for each instance, in order. On any failure,
/// every already-staged candidate (including the one that just failed, if
/// `enable` is what failed) is torn down through `destroy` before returning
/// `None`.
fn stage_composition_entries<Runtime, const OVERLAYS: usize>(
    runtime: &mut Runtime,
    instances: &[OverlayInstance],
    blocked: &mut Vec<Runtime::Instance, OVERLAYS>,
) -> Option<Vec<Runtime::Instance, OVERLAYS>>
where
    Runtime: CompositionRuntime,
{
    let mut candidates: Vec<Runtime::Instance, OVERLAYS> = Vec::new();
    for instance in instances {
        let candidate = match runtime.enter(*instance) {
            Ok(candidate) => candidate,
            Err(_) => {
                destroy_composition_instances(runtime, candidates, blocked);
                return None;
            }
        };
        let ready = runtime.enable(&candidate);
        if let Err(candidate) = candidates.push(candidate) {
            // Capacity exceeded: the just-entered candidate never joined
            // `candidates` -- destroy it directly, then roll back everything
            // staged before it.
            destroy_one_composition_instance(runtime, candidate, blocked);
            destroy_composition_instances(runtime, candidates, blocked);
            return None;
        }
        if !ready {
            destroy_composition_instances(runtime, candidates, blocked);
            return None;
        }
    }
    Some(candidates)
}

/// Owns [`ShellModel`] plus the visible screen and overlay instances it
/// governs, and drives their lifecycle transactions against a product's
/// [`SurfaceRuntime`] (screens) and [`CompositionRuntime`] (overlays)
/// target adapters. Renderer-independent: nothing here names LVGL, a board,
/// or a product.
pub struct UiCoordinator<
    Screen,
    Overlay,
    const PROVIDERS: usize,
    const SURFACES: usize,
    const NAVIGATION: usize,
    const OVERLAYS: usize,
    const MODALS: usize,
    const INTENTS: usize,
> {
    shell: ShellModel<PROVIDERS, SURFACES, NAVIGATION, OVERLAYS, MODALS, INTENTS>,
    active_screen: Option<Screen>,
    cleanup_blocked_screen: Option<Screen>,
    live_overlays: Vec<Overlay, OVERLAYS>,
    overlay_cleanup_blocked: Vec<Overlay, OVERLAYS>,
    navigation_faulted: bool,
    composition_faulted: bool,
    lifecycle_audit_faulted: bool,
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
    pub fn new(
        base_provider: ProviderId,
        surfaces: &[SurfaceSpec],
        fallback_id: SurfaceId,
    ) -> Result<Self, ShellInitError> {
        Ok(Self {
            shell: ShellModel::new(base_provider, surfaces, fallback_id)?,
            active_screen: None,
            cleanup_blocked_screen: None,
            live_overlays: Vec::new(),
            overlay_cleanup_blocked: Vec::new(),
            navigation_faulted: false,
            composition_faulted: false,
            lifecycle_audit_faulted: false,
        })
    }

    pub fn register_provider(
        &mut self,
        provider: ProviderId,
        surfaces: &[SurfaceSpec],
    ) -> Result<ProviderToken, ProviderRegistrationError> {
        self.shell.register_provider(provider, surfaces)
    }

    pub fn shell(&self) -> &ShellModel<PROVIDERS, SURFACES, NAVIGATION, OVERLAYS, MODALS, INTENTS> {
        &self.shell
    }

    /// Queues a navigation intent for later draining rather than dispatching
    /// it immediately -- pure passthrough to `ShellModel`, for callers that
    /// enqueue from a context [`Self::dispatch_navigation`] cannot safely run
    /// in directly (an LVGL event callback still inside `lv_timer_handler`,
    /// say) and drain afterward. Source ownership and staleness are checked
    /// at enqueue time, same as `ShellModel::queue_intent`.
    pub fn queue_intent(&mut self, intent: OwnedNavIntent) -> Result<(), IntentQueueError> {
        self.shell.queue_intent(intent)
    }

    /// Pops the next queued navigation intent, for a drain loop to dispatch
    /// via [`Self::dispatch_navigation`] -- pure passthrough to `ShellModel`.
    pub fn pop_intent(&mut self) -> Option<OwnedNavIntent> {
        self.shell.pop_intent()
    }

    /// The coordinator refuses new dispatches while a fault is latched or a
    /// cleanup remains blocked -- matching `Backend::drain_shell_navigation`'s
    /// existing gate -- until [`Self::retry_blocked_cleanup`] clears it.
    pub fn is_faulted(&self) -> bool {
        self.navigation_faulted
            || self.composition_faulted
            || self.lifecycle_audit_faulted
            || self.cleanup_blocked_screen.is_some()
            || !self.overlay_cleanup_blocked.is_empty()
    }

    /// Enters, activates and enables the initial screen directly (no origin
    /// to roll back to). Call once, before any [`Self::dispatch_navigation`],
    /// matching `Backend::initialize_into`'s bootstrap of the ambient home
    /// screen outside any transition.
    ///
    /// An activation or enable failure has nowhere to roll back to either --
    /// the candidate is destroyed on a best-effort basis and, if that itself
    /// fails, retained as [`Self::cleanup_blocked_screen`] with the
    /// coordinator left faulted (see [`Self::is_faulted`]) rather than left
    /// silently unowned, matching every other lifecycle failure path's "never
    /// drop a live instance" invariant.
    pub fn bootstrap_screen<Runtime>(
        &mut self,
        runtime: &mut Runtime,
    ) -> Result<(), BootstrapError<Runtime::EnterError>>
    where
        Runtime: SurfaceRuntime<Instance = Screen>,
    {
        debug_assert!(self.active_screen.is_none(), "bootstrap runs exactly once");
        let candidate = runtime
            .enter(self.shell.active(), self.shell.active_instance())
            .map_err(BootstrapError::Enter)?;
        if !runtime.activate(&candidate) {
            self.destroy_orphaned_candidate(runtime, candidate);
            return Err(BootstrapError::Activation);
        }
        if !runtime.enable(&candidate) {
            self.destroy_orphaned_candidate(runtime, candidate);
            return Err(BootstrapError::Enable);
        }
        self.active_screen = Some(candidate);
        Ok(())
    }

    /// Installs an already-live screen instance as the initial active
    /// screen, bypassing [`Self::bootstrap_screen`]'s own enter/activate/
    /// enable sequence -- for a product whose bring-up has its own extra
    /// ceremony around entering the first screen (swapping away a renderer's
    /// implicitly-created default screen before deleting it, say) that
    /// [`SurfaceRuntime`]'s enter/activate/enable/destroy shape cannot
    /// express on its own. The caller is responsible for `instance` already
    /// being fully entered, activated, and enabled.
    pub fn install_bootstrap_screen(&mut self, instance: Screen) {
        debug_assert!(self.active_screen.is_none(), "bootstrap runs exactly once");
        self.active_screen = Some(instance);
    }

    /// Destroys a screen candidate that was constructed but never put into
    /// service (a bootstrap activate/enable failure, or a provider-removal
    /// fallback candidate abandoned before or after the shell commit). There
    /// is no origin to roll back to for these -- the candidate is destroyed
    /// on a best-effort basis and, on either failure kind, the coordinator
    /// is left faulted (see [`Self::is_faulted`]) rather than silently
    /// dropping a live instance or an audit mismatch.
    fn destroy_orphaned_candidate<Runtime>(&mut self, runtime: &mut Runtime, candidate: Screen)
    where
        Runtime: SurfaceRuntime<Instance = Screen>,
    {
        match runtime.destroy(candidate) {
            Ok(()) => {}
            Err(DestroyFailure::Live(candidate)) => {
                self.cleanup_blocked_screen = Some(candidate);
                self.navigation_faulted = true;
            }
            Err(DestroyFailure::Audit) => {
                self.lifecycle_audit_faulted = true;
                self.navigation_faulted = true;
            }
        }
    }

    /// Reports whether the active screen instance and `ShellModel`'s
    /// navigation state agree -- the same alignment check
    /// `Backend::active_surface_is_renderable` performs, generalized past
    /// the LVGL-specific validity check a real renderer adds on top.
    pub fn active_screen_ready(&self) -> bool {
        self.active_screen.is_some() && !self.is_faulted()
    }

    /// The surface instance that should currently own exclusive input: the
    /// active modal if one is live, otherwise the active screen. Not an
    /// independently tracked field -- derived fresh from `ShellModel` every
    /// call, the same way `Backend::sync_overlay_visibility_for_active_surface`
    /// derives visibility from `shell.active()` rather than caching it.
    pub fn focus(&self) -> SurfaceInstanceToken {
        self.shell
            .active_modal()
            .map_or(self.shell.active_instance(), |modal| modal.token)
    }

    pub fn merged_refresh_hint(&self) -> RefreshHint {
        self.shell.merged_refresh_hint()
    }

    pub fn live_overlay_len(&self) -> usize {
        self.live_overlays.len()
    }

    pub fn overlay_cleanup_blocked_len(&self) -> usize {
        self.overlay_cleanup_blocked.len()
    }

    /// The active screen instance, for reads/updates that are not themselves
    /// a lifecycle transition (per-frame polling, a background-tap query) --
    /// the same reason [`Self::live_overlay`] exists for overlays. Prefer
    /// [`Self::dispatch_navigation`] for anything that changes which screen
    /// is active.
    pub fn active_screen(&self) -> Option<&Screen> {
        self.active_screen.as_ref()
    }

    /// The mutable counterpart of [`Self::active_screen`].
    pub fn active_screen_mut(&mut self) -> Option<&mut Screen> {
        self.active_screen.as_mut()
    }

    /// The screen retained in a cleanup fault (if any), for the same kind of
    /// read-only status reporting [`Self::active_screen`] exists for.
    pub fn cleanup_blocked_screen(&self) -> Option<&Screen> {
        self.cleanup_blocked_screen.as_ref()
    }

    /// Every live overlay, for read-only per-product policy the coordinator
    /// doesn't itself decide (which overlays should currently be visible,
    /// say) -- [`Self::commit_composition_transaction`] already owns
    /// `show`/`hide` during the transition itself; this is for later,
    /// independent visibility policy.
    pub fn live_overlays(&self) -> impl Iterator<Item = &Overlay> {
        self.live_overlays.iter()
    }

    /// The mutable counterpart of [`Self::live_overlays`].
    pub fn live_overlays_mut(&mut self) -> impl Iterator<Item = &mut Overlay> {
        self.live_overlays.iter_mut()
    }

    /// Every overlay retained in a cleanup fault, for the same kind of
    /// read-only product-policy queries [`Self::live_overlays`] exists for
    /// (a modal-presence check that must also see instances stuck mid-retry,
    /// say).
    pub fn overlay_cleanup_blocked(&self) -> impl Iterator<Item = &Overlay> {
        self.overlay_cleanup_blocked.iter()
    }

    pub fn navigation_faulted(&self) -> bool {
        self.navigation_faulted
    }

    pub fn composition_faulted(&self) -> bool {
        self.composition_faulted
    }

    pub fn lifecycle_audit_faulted(&self) -> bool {
        self.lifecycle_audit_faulted
    }

    /// Escape hatch for fault conditions only the product can detect (an
    /// external audit finding a stale callback route, say) -- the
    /// transactional `dispatch_*` methods already set/clear these correctly
    /// for every fault they themselves can detect; this exists for the rest.
    pub fn set_navigation_faulted(&mut self, faulted: bool) {
        self.navigation_faulted = faulted;
    }

    pub fn set_composition_faulted(&mut self, faulted: bool) {
        self.composition_faulted = faulted;
    }

    pub fn set_lifecycle_audit_faulted(&mut self, faulted: bool) {
        self.lifecycle_audit_faulted = faulted;
    }

    /// Consumes the coordinator and returns everything it owned -- the
    /// active and cleanup-blocked screens, and every live and
    /// cleanup-blocked overlay -- for a caller that must tear a partially
    /// built coordinator back down after its own bring-up failed partway
    /// through (there is no origin to roll back to yet, so no
    /// [`SurfaceRuntime`]/[`CompositionRuntime`] transition applies).
    /// Matches `Backend::destroy_initial_backend_or_stop`'s existing
    /// teardown, generalized past LVGL.
    pub fn into_owned_instances(
        self,
    ) -> (
        Option<Screen>,
        Option<Screen>,
        Vec<Overlay, OVERLAYS>,
        Vec<Overlay, OVERLAYS>,
    ) {
        (
            self.active_screen,
            self.cleanup_blocked_screen,
            self.live_overlays,
            self.overlay_cleanup_blocked,
        )
    }

    /// The live overlay instance for `token`, if one is currently admitted --
    /// for per-frame content updates (a status label's text, say) that are
    /// not themselves a lifecycle transition and so don't go through
    /// [`Self::dispatch_composition`]. Takes `runtime` only to resolve each
    /// live instance's [`OverlayInstance`] via [`CompositionRuntime::instance`]
    /// -- the coordinator does not otherwise retain a runtime between calls.
    pub fn live_overlay<Runtime>(
        &self,
        token: SurfaceInstanceToken,
        runtime: &Runtime,
    ) -> Option<&Overlay>
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        self.live_overlays
            .iter()
            .find(|live| runtime.instance(live).token == token)
    }

    /// The mutable counterpart of [`Self::live_overlay`].
    pub fn live_overlay_mut<Runtime>(
        &mut self,
        token: SurfaceInstanceToken,
        runtime: &Runtime,
    ) -> Option<&mut Overlay>
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        self.live_overlays
            .iter_mut()
            .find(|live| runtime.instance(live).token == token)
    }

    /// Stages `disable`+`hide` for every overlay named in `tokens`, in
    /// order. Shared by every dispatch path that removes overlays
    /// ([`overlays`], [`navigation`], [`provider_removal`]) -- kept here,
    /// not in any one of them, for exactly that reason.
    fn disable_live<Runtime>(&self, tokens: &[SurfaceInstanceToken], runtime: &mut Runtime) -> bool
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        for (staged, token) in tokens.iter().enumerate() {
            let Some(index) = self
                .live_overlays
                .iter()
                .position(|live| runtime.instance(live).token == *token)
            else {
                // Best-effort undo of this attempt's own staged disables;
                // its result folds into the caller's own rollback report,
                // since every caller here re-disables the same tokens on
                // its next attempt regardless of whether this undo was
                // itself complete.
                let _ = self.enable_live(&tokens[..staged], runtime);
                return false;
            };
            if !runtime.disable(&self.live_overlays[index]) {
                let _ = self.enable_live(&tokens[..staged], runtime);
                return false;
            }
            runtime.hide(&self.live_overlays[index]);
        }
        true
    }

    /// Re-shows and re-enables every overlay in `tokens` that
    /// [`Self::disable_live`] had staged for departure, for a caller rolling
    /// that staging back after a later step failed. Returns whether every
    /// one of them was found live and its `runtime.enable()` call actually
    /// succeeded -- the same "did the rollback really restore what it was
    /// undoing" bookkeeping `execute_transition`'s `origin_restored` does
    /// for screens. A caller rolling back a whole transaction folds a
    /// `false` here into `composition_faulted` rather than reporting a
    /// clean rollback that left an overlay shown but not actually enabled.
    fn enable_live<Runtime>(&self, tokens: &[SurfaceInstanceToken], runtime: &mut Runtime) -> bool
    where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        let mut all_restored = true;
        for token in tokens {
            match self
                .live_overlays
                .iter()
                .position(|live| runtime.instance(live).token == *token)
            {
                Some(index) => {
                    runtime.show(&self.live_overlays[index]);
                    if !runtime.enable(&self.live_overlays[index]) {
                        all_restored = false;
                    }
                }
                None => all_restored = false,
            }
        }
        all_restored
    }

    fn absorb_blocked_overlays(&mut self, mut blocked: Vec<Overlay, OVERLAYS>, audit_failed: bool) {
        while let Some(instance) = blocked.pop() {
            // The registry is bounded by the same capacity `ShellModel`
            // itself enforces for live overlays, so every blocked instance
            // (never more than were live) fits.
            let _ = self.overlay_cleanup_blocked.push(instance);
        }
        if !self.overlay_cleanup_blocked.is_empty() {
            self.navigation_faulted = true;
        }
        self.composition_faulted |= audit_failed;
        self.navigation_faulted |= audit_failed;
    }

    /// Post-commit completion, shared by composition and navigation
    /// dispatch: destroy every departed overlay (already disabled/hidden by
    /// staging) and remove it from `live_overlays`, then `show` every
    /// entered candidate and add it. Only ever called after `ShellModel`'s
    /// own commit succeeded, matching `Backend::complete_overlay_departures`
    /// + `activate_overlay_entries`.
    fn complete_composition<Runtime>(
        &mut self,
        entered: Vec<Overlay, OVERLAYS>,
        departing_tokens: &[SurfaceInstanceToken],
        runtime: &mut Runtime,
    ) where
        Runtime: CompositionRuntime<Instance = Overlay>,
    {
        let mut audit_failed = false;
        for token in departing_tokens.iter().copied() {
            if let Some(index) = self
                .live_overlays
                .iter()
                .position(|live| runtime.instance(live).token == token)
            {
                let removed = self.live_overlays.remove(index);
                match runtime.destroy(removed) {
                    Ok(()) => {}
                    Err(DestroyFailure::Live(removed)) => {
                        let _ = self.overlay_cleanup_blocked.push(removed);
                        self.navigation_faulted = true;
                    }
                    Err(DestroyFailure::Audit) => audit_failed = true,
                }
            }
        }
        for candidate in entered {
            let instance = runtime.instance(&candidate);
            if instance.input == super::types::OverlayInput::Modal {
                runtime.set_exclusive_capture(true);
            }
            runtime.show(&candidate);
            let _ = self.live_overlays.push(candidate);
        }
        self.composition_faulted |= audit_failed;
        self.navigation_faulted |= audit_failed;
    }

    /// The single entry point a product's event loop drives: dispatches
    /// whichever shared action `event` carries, or reports a product
    /// payload as ignored (see [`UiEvent`]'s doc for why the coordinator
    /// never interprets one itself).
    pub fn dispatch<P, ScreenRt, OverlayRt>(
        &mut self,
        event: UiEvent<P>,
        screen_runtime: &mut ScreenRt,
        overlay_runtime: &mut OverlayRt,
    ) -> UiDispatchResult
    where
        ScreenRt: SurfaceRuntime<Instance = Screen>,
        OverlayRt: CompositionRuntime<Instance = Overlay>,
    {
        match event {
            UiEvent::Navigate(intent) => UiDispatchResult::Navigation(self.dispatch_navigation(
                intent,
                screen_runtime,
                overlay_runtime,
            )),
            UiEvent::Compose(owned) => {
                UiDispatchResult::Composition(self.dispatch_composition(owned, overlay_runtime))
            }
            UiEvent::Product(_) => UiDispatchResult::ProductIgnored,
        }
    }
}
