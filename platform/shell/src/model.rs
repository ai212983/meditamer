use core::mem::size_of;

#[cfg(all(test, not(target_os = "none")))]
use super::types::OverlayDismissal;
use super::{
    composition::{
        CompositionDelta, CompositionPlan, CompositionPlanResult, CompositionPurge,
        CompositionReferenceError, CompositionReferences,
    },
    intent_queue::{IntentQueue, IntentQueueError},
    navigator::{
        NavigationError, NavigationFrame, NavigationOutcome, NavigationPlan, NavigationPurge,
        Navigator,
    },
    registry::{RegistrationError, ResolveError, SurfaceRegistry},
    types::{
        CompositionIntent, InstanceGeneration, NavIntent, OverlayAdmission, OverlayBand,
        OverlayInput, OverlayInstance, OverlayLifetime, OwnedCompositionIntent, OwnedNavIntent,
        ProviderId, ProviderToken, RefreshHint, SurfaceCapabilities, SurfaceId,
        SurfaceInstanceToken, SurfaceRef, SurfaceRole, SurfaceSpec,
    },
};

pub const PROVIDER_CAPACITY: usize = 8;
pub const SURFACE_REGISTRY_CAPACITY: usize = 16;
pub const NAVIGATION_STACK_CAPACITY: usize = 8;
pub const LIVE_OVERLAY_CAPACITY: usize = 4;
pub const MODAL_QUEUE_CAPACITY: usize = 4;
pub const SHELL_INTENT_QUEUE_CAPACITY: usize = 8;
pub const RETAINED_MODEL_CAPACITY: usize = 0;
pub const FUTURE_RETAINED_MODEL_REFERENCE_CEILING: usize = 4;

pub type DefaultShellModel = ShellModel<
    PROVIDER_CAPACITY,
    SURFACE_REGISTRY_CAPACITY,
    NAVIGATION_STACK_CAPACITY,
    LIVE_OVERLAY_CAPACITY,
    MODAL_QUEUE_CAPACITY,
    SHELL_INTENT_QUEUE_CAPACITY,
>;

pub const DEFAULT_SHELL_MODEL_BYTES: usize = size_of::<DefaultShellModel>();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceContractError {
    MissingCapability {
        id: super::types::SurfaceId,
        role: SurfaceRole,
        required: SurfaceCapabilities,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellInitError {
    AmbientRole,
    SurfaceContract(SurfaceContractError),
    Registration(RegistrationError),
    Resolve(ResolveError),
    Navigation(NavigationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderRegistrationError {
    ProviderRemovalInProgress,
    SurfaceContract(SurfaceContractError),
    Registration(RegistrationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShellNavigationError {
    ProviderRemovalInProgress,
    Resolve(ResolveError),
    Navigation(NavigationError),
    InstanceGenerationExhausted,
    StalePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionError {
    ProviderRemovalInProgress,
    Resolve(ResolveError),
    NotOverlay(SurfaceRef),
    InvalidSource(SurfaceInstanceToken),
    UnknownInstance(SurfaceInstanceToken),
    InstanceGenerationExhausted,
    StalePlan,
    Reference(CompositionReferenceError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderRemovalError {
    RemovalInProgress,
    Resolve(ResolveError),
    OwnsAmbientRoot,
    InstanceGenerationExhausted,
    StalePlan,
    ReferencesRemain,
    RuntimeAuditMismatch,
    Navigation(NavigationError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderPurge {
    pub definitions: usize,
    pub navigation: NavigationPurge,
    pub composition: CompositionPurge,
    pub queued_intents: usize,
}

pub struct PreparedNavigation<
    const CAPACITY: usize,
    const LIVE_CAPACITY: usize,
    const MODAL_CAPACITY: usize,
> {
    expected_shell_revision: u32,
    plan: NavigationPlan<CAPACITY>,
    destination_instance: SurfaceInstanceToken,
    composition_plan: CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>,
}

impl<const CAPACITY: usize, const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    PreparedNavigation<CAPACITY, LIVE_CAPACITY, MODAL_CAPACITY>
{
    pub fn origin(&self) -> NavigationFrame {
        self.plan.origin()
    }

    pub fn destination(&self) -> NavigationFrame {
        self.plan.destination()
    }

    pub fn requires_reentry(&self, active_instance: SurfaceInstanceToken) -> bool {
        self.destination_instance != active_instance
    }

    pub fn destination_instance(&self) -> SurfaceInstanceToken {
        self.destination_instance
    }

    pub fn composition_delta(&self) -> &CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY> {
        self.composition_plan.delta()
    }
}

pub struct PreparedComposition<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize> {
    expected_shell_revision: u32,
    plan: CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>,
}

impl<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    PreparedComposition<LIVE_CAPACITY, MODAL_CAPACITY>
{
    pub fn result(&self) -> CompositionPlanResult {
        self.plan.result()
    }

    pub fn delta(&self) -> &CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY> {
        self.plan.delta()
    }

    pub fn requires_entry(&self) -> bool {
        !self.plan.delta().enter_live().is_empty()
    }
}

pub struct ProviderRemovalPlan<
    const NAVIGATION_CAPACITY: usize,
    const LIVE_CAPACITY: usize,
    const MODAL_CAPACITY: usize,
> {
    expected_shell_revision: u32,
    owner: ProviderToken,
    fallback: Option<NavigationPlan<NAVIGATION_CAPACITY>>,
    fallback_instance: Option<SurfaceInstanceToken>,
    composition_plan: CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>,
}

impl<const NAVIGATION_CAPACITY: usize, const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    ProviderRemovalPlan<NAVIGATION_CAPACITY, LIVE_CAPACITY, MODAL_CAPACITY>
{
    pub const fn owner(&self) -> ProviderToken {
        self.owner
    }

    pub fn fallback_transition(&self) -> Option<ProviderFallbackTransition> {
        Some(ProviderFallbackTransition {
            origin: self.fallback.as_ref()?.origin(),
            destination: self.fallback.as_ref()?.destination(),
            destination_instance: self.fallback_instance?,
        })
    }

    pub fn composition_delta(&self) -> &CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY> {
        self.composition_plan.delta()
    }
}

pub struct PendingProviderRemoval {
    expected_shell_revision: u32,
    owner: ProviderToken,
    navigation: NavigationPurge,
    composition: CompositionPurge,
    queued_intents: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderRuntimeAudit {
    owner: ProviderToken,
}

impl ProviderRuntimeAudit {
    pub const fn verified(owner: ProviderToken) -> Self {
        Self { owner }
    }
}

impl PendingProviderRemoval {
    pub const fn owner(&self) -> ProviderToken {
        self.owner
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderFallbackTransition {
    pub origin: NavigationFrame,
    pub destination: NavigationFrame,
    pub destination_instance: SurfaceInstanceToken,
}

pub struct ShellModel<
    const PROVIDERS: usize,
    const SURFACES: usize,
    const NAVIGATION: usize,
    const OVERLAYS: usize,
    const MODALS: usize,
    const INTENTS: usize,
> {
    registry: SurfaceRegistry<PROVIDERS, SURFACES>,
    navigator: Navigator<NAVIGATION>,
    composition: CompositionReferences<OVERLAYS, MODALS>,
    intents: IntentQueue<INTENTS>,
    base_owner: ProviderToken,
    pending_provider_removal: Option<ProviderToken>,
    active_instance: SurfaceInstanceToken,
    next_instance_generation: u32,
    revision: u32,
}

impl<
        const PROVIDERS: usize,
        const SURFACES: usize,
        const NAVIGATION: usize,
        const OVERLAYS: usize,
        const MODALS: usize,
        const INTENTS: usize,
    > ShellModel<PROVIDERS, SURFACES, NAVIGATION, OVERLAYS, MODALS, INTENTS>
{
    pub fn new(
        base_provider: ProviderId,
        surfaces: &[SurfaceSpec],
        fallback_id: SurfaceId,
    ) -> Result<Self, ShellInitError> {
        let fallback = surfaces
            .iter()
            .find(|surface| surface.id == fallback_id)
            .copied()
            .ok_or(ShellInitError::Registration(
                RegistrationError::EmptyProvider,
            ))?;
        if fallback.role != SurfaceRole::Ambient {
            return Err(ShellInitError::AmbientRole);
        }
        for surface in surfaces {
            validate_surface(*surface).map_err(ShellInitError::SurfaceContract)?;
        }

        let mut registry = SurfaceRegistry::new();
        let owner = registry
            .register_provider(base_provider, surfaces)
            .map_err(ShellInitError::Registration)?;
        let fallback_ref = SurfaceRef {
            owner,
            id: fallback.id,
        };
        let fallback_definition = *registry
            .resolve(fallback_ref)
            .map_err(ShellInitError::Resolve)?;
        let navigator = Navigator::new(fallback_definition).map_err(ShellInitError::Navigation)?;
        let active_instance = SurfaceInstanceToken::issued(fallback_ref, InstanceGeneration(1));
        Ok(Self {
            registry,
            navigator,
            composition: CompositionReferences::new(),
            intents: IntentQueue::new(),
            base_owner: owner,
            pending_provider_removal: None,
            active_instance,
            next_instance_generation: 2,
            revision: 0,
        })
    }

    pub fn register_provider(
        &mut self,
        provider: ProviderId,
        surfaces: &[SurfaceSpec],
    ) -> Result<ProviderToken, ProviderRegistrationError> {
        if self.pending_provider_removal.is_some() {
            return Err(ProviderRegistrationError::ProviderRemovalInProgress);
        }
        for surface in surfaces {
            validate_surface(*surface).map_err(ProviderRegistrationError::SurfaceContract)?;
        }
        let token = self
            .registry
            .register_provider(provider, surfaces)
            .map_err(ProviderRegistrationError::Registration)?;
        self.revision = self.revision.wrapping_add(1);
        Ok(token)
    }

    pub fn prepare_intent(
        &mut self,
        intent: NavIntent,
    ) -> Result<PreparedNavigation<NAVIGATION, OVERLAYS, MODALS>, ShellNavigationError> {
        if self.pending_provider_removal.is_some() {
            return Err(ShellNavigationError::ProviderRemovalInProgress);
        }
        let definition = match intent {
            NavIntent::OpenLauncher(surface)
            | NavIntent::Launch(surface)
            | NavIntent::Push(surface) => Some(
                *self
                    .registry
                    .resolve(surface)
                    .map_err(ShellNavigationError::Resolve)?,
            ),
            NavIntent::Back | NavIntent::Home => None,
        };

        let plan = match (intent, definition) {
            (NavIntent::OpenLauncher(_), Some(definition)) => {
                self.navigator.prepare_open_launcher(definition)
            }
            (NavIntent::Launch(_), Some(definition)) => self.navigator.prepare_launch(definition),
            (NavIntent::Push(_), Some(definition)) => self.navigator.prepare_push_child(definition),
            (NavIntent::Back, None) => Ok(self.navigator.prepare_back()),
            (NavIntent::Home, None) => Ok(self.navigator.prepare_home()),
            _ => unreachable!("intent and resolved definition must agree"),
        }
        .map_err(ShellNavigationError::Navigation)?;
        let destination = plan.destination();
        let destination_instance = if destination.surface == self.active_instance.surface {
            self.active_instance
        } else {
            self.issue_instance(destination.surface)?
        };
        Ok(PreparedNavigation {
            expected_shell_revision: self.revision,
            plan,
            destination_instance,
            composition_plan: self.composition.plan_drop_transient(),
        })
    }

    pub fn commit_navigation(
        &mut self,
        prepared: PreparedNavigation<NAVIGATION, OVERLAYS, MODALS>,
    ) -> Result<NavigationOutcome, ShellNavigationError> {
        if self.pending_provider_removal.is_some() {
            return Err(ShellNavigationError::ProviderRemovalInProgress);
        }
        if prepared.expected_shell_revision != self.revision {
            return Err(ShellNavigationError::StalePlan);
        }
        let instance_changed = prepared.destination_instance != self.active_instance;
        let destination_instance = prepared.destination_instance;
        let composition_changed = !prepared.composition_plan.delta().is_empty();
        let outcome = self
            .navigator
            .commit(prepared.plan)
            .map_err(ShellNavigationError::Navigation)?;
        let (next_composition, composition_delta, _) = prepared.composition_plan.into_parts();
        for instance in composition_delta
            .leave_live()
            .iter()
            .chain(composition_delta.remove_queued())
        {
            self.intents.purge_instance(instance.token);
        }
        self.composition = next_composition;
        if instance_changed {
            let previous = self.active_instance;
            self.active_instance = destination_instance;
            self.intents.purge_instance(previous);
        }
        if outcome == NavigationOutcome::Changed || instance_changed || composition_changed {
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(outcome)
    }

    pub fn prepare_recovery_home(
        &mut self,
    ) -> Result<PreparedNavigation<NAVIGATION, OVERLAYS, MODALS>, ShellNavigationError> {
        if self.pending_provider_removal.is_some() {
            return Err(ShellNavigationError::ProviderRemovalInProgress);
        }
        let plan = self.navigator.prepare_home();
        let destination_instance = self.issue_instance(plan.destination().surface)?;
        Ok(PreparedNavigation {
            expected_shell_revision: self.revision,
            plan,
            destination_instance,
            composition_plan: self.composition.plan_drop_transient(),
        })
    }

    pub fn prepare_overlay_request(
        &mut self,
        surface: SurfaceRef,
        input: OverlayInput,
        lifetime: OverlayLifetime,
        rank: u8,
    ) -> Result<PreparedComposition<OVERLAYS, MODALS>, CompositionError> {
        self.prepare_overlay_request_for(surface.owner, surface, input, lifetime, rank)
    }

    fn prepare_overlay_request_for(
        &mut self,
        request_owner: ProviderToken,
        surface: SurfaceRef,
        input: OverlayInput,
        lifetime: OverlayLifetime,
        rank: u8,
    ) -> Result<PreparedComposition<OVERLAYS, MODALS>, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        self.validate_overlay(surface)?;
        let token = self
            .issue_instance(surface)
            .map_err(|_| CompositionError::InstanceGenerationExhausted)?;
        let instance = OverlayInstance {
            token,
            request_owner,
            band: if surface.owner == self.base_owner {
                OverlayBand::BaseSystem
            } else {
                OverlayBand::Provider
            },
            input,
            lifetime,
            rank,
        };
        let plan = self
            .composition
            .plan_admission(instance)
            .map_err(CompositionError::Reference)?;
        Ok(PreparedComposition {
            expected_shell_revision: self.revision,
            plan,
        })
    }

    pub fn prepare_composition_intent(
        &mut self,
        owned: OwnedCompositionIntent,
    ) -> Result<PreparedComposition<OVERLAYS, MODALS>, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        match owned.intent {
            CompositionIntent::Request {
                surface,
                input,
                lifetime,
                rank,
            } => {
                if owned.source != self.active_instance {
                    return Err(CompositionError::InvalidSource(owned.source));
                }
                self.prepare_overlay_request_for(
                    owned.source.surface.owner,
                    surface,
                    input,
                    lifetime,
                    rank,
                )
            }
            CompositionIntent::DismissActiveModal => {
                if self
                    .composition
                    .active_modal()
                    .is_none_or(|modal| modal.token != owned.source)
                {
                    return Err(CompositionError::InvalidSource(owned.source));
                }
                self.prepare_overlay_removal(owned.source)
            }
        }
    }

    pub fn commit_overlay_request(
        &mut self,
        prepared: PreparedComposition<OVERLAYS, MODALS>,
    ) -> Result<OverlayAdmission, CompositionError> {
        match self.commit_composition(prepared)? {
            CompositionPlanResult::Admission(admission) => Ok(admission),
            CompositionPlanResult::Removal(_) | CompositionPlanResult::Cleanup(_) => {
                unreachable!("overlay admission plans always return admission results")
            }
        }
    }

    pub fn prepare_overlay_removal(
        &self,
        token: SurfaceInstanceToken,
    ) -> Result<PreparedComposition<OVERLAYS, MODALS>, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        let plan = self
            .composition
            .plan_removal(token)
            .ok_or(CompositionError::UnknownInstance(token))?;
        Ok(PreparedComposition {
            expected_shell_revision: self.revision,
            plan,
        })
    }

    pub fn prepare_transient_overlay_removal(
        &self,
    ) -> Result<Option<PreparedComposition<OVERLAYS, MODALS>>, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        if self.composition.first_transient().is_none() {
            return Ok(None);
        }
        Ok(Some(PreparedComposition {
            expected_shell_revision: self.revision,
            plan: self.composition.plan_drop_transient(),
        }))
    }

    pub fn commit_overlay_removal(
        &mut self,
        prepared: PreparedComposition<OVERLAYS, MODALS>,
    ) -> Result<CompositionPlanResult, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        self.commit_composition(prepared)
    }

    pub fn commit_composition(
        &mut self,
        prepared: PreparedComposition<OVERLAYS, MODALS>,
    ) -> Result<CompositionPlanResult, CompositionError> {
        if self.pending_provider_removal.is_some() {
            return Err(CompositionError::ProviderRemovalInProgress);
        }
        if prepared.expected_shell_revision != self.revision {
            return Err(CompositionError::StalePlan);
        }
        let (next, delta, result) = prepared.plan.into_parts();
        for instance in delta.leave_live().iter().chain(delta.remove_queued()) {
            self.intents.purge_instance(instance.token);
        }
        self.composition = next;
        self.revision = self.revision.wrapping_add(1);
        Ok(result)
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub fn request_overlay(
        &mut self,
        surface: SurfaceRef,
        input: OverlayInput,
        lifetime: OverlayLifetime,
        rank: u8,
    ) -> Result<OverlayAdmission, CompositionError> {
        let prepared = self.prepare_overlay_request(surface, input, lifetime, rank)?;
        self.commit_overlay_request(prepared)
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub fn remove_overlay(&mut self, token: SurfaceInstanceToken) -> Option<OverlayDismissal> {
        let prepared = self.prepare_overlay_removal(token).ok()?;
        match self.commit_overlay_removal(prepared).ok()? {
            CompositionPlanResult::Removal(dismissal) => Some(dismissal),
            CompositionPlanResult::Admission(_) | CompositionPlanResult::Cleanup(_) => None,
        }
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub fn dismiss_active_modal(&mut self) -> Option<OverlayDismissal> {
        let active = self.composition.active_modal()?;
        self.remove_overlay(active.token)
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub fn drop_transient_overlays(&mut self) -> CompositionPurge {
        let mut purge = CompositionPurge::default();
        if let Some(prepared) = self
            .prepare_transient_overlay_removal()
            .expect("the test-only helper cannot run during provider removal")
        {
            let result = self
                .commit_overlay_removal(prepared)
                .expect("the test-only helper commits immediately");
            if let CompositionPlanResult::Cleanup(cleanup) = result {
                purge = cleanup;
            }
        }
        purge
    }

    pub fn queue_intent(&mut self, intent: OwnedNavIntent) -> Result<(), IntentQueueError> {
        if self.pending_provider_removal.is_some() {
            return Err(IntentQueueError::ProviderRemovalInProgress);
        }
        // Source ownership is checked at enqueue time. Target ownership is resolved
        // when the display-task owner drains and prepares the intent.
        if self
            .registry
            .validate_owner(intent.source.surface.owner)
            .is_err()
        {
            return Err(IntentQueueError::InvalidOwner);
        }
        if intent.source != self.active_instance {
            return Err(IntentQueueError::StaleInstance);
        }
        self.intents.push(intent)?;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn pop_intent(&mut self) -> Option<OwnedNavIntent> {
        if self.pending_provider_removal.is_some() {
            return None;
        }
        let intent = self.intents.pop();
        if intent.is_some() {
            self.revision = self.revision.wrapping_add(1);
        }
        intent
    }

    pub fn prepare_provider_removal(
        &mut self,
        owner: ProviderToken,
    ) -> Result<ProviderRemovalPlan<NAVIGATION, OVERLAYS, MODALS>, ProviderRemovalError> {
        if self.pending_provider_removal.is_some() {
            return Err(ProviderRemovalError::RemovalInProgress);
        }
        self.registry
            .validate_owner(owner)
            .map_err(ProviderRemovalError::Resolve)?;
        if self.navigator.owns_ambient(owner) {
            return Err(ProviderRemovalError::OwnsAmbientRoot);
        }
        let fallback = self
            .navigator
            .references_provider(owner)
            .then(|| self.navigator.prepare_home());
        let fallback_instance = match fallback.as_ref() {
            Some(plan) if plan.destination().surface != self.active_instance.surface => Some(
                self.issue_instance(plan.destination().surface)
                    .map_err(|_| ProviderRemovalError::InstanceGenerationExhausted)?,
            ),
            _ => None,
        };
        let drop_transients = fallback.is_some();
        Ok(ProviderRemovalPlan {
            expected_shell_revision: self.revision,
            owner,
            fallback,
            fallback_instance,
            composition_plan: self
                .composition
                .plan_provider_detach(owner, drop_transients),
        })
    }

    // Detach removes all shell/runtime references but deliberately retains the
    // provider registry record until the display-task owner proves synchronous
    // runtime cleanup and callback-route audits.
    pub fn commit_provider_detach(
        &mut self,
        plan: ProviderRemovalPlan<NAVIGATION, OVERLAYS, MODALS>,
    ) -> Result<PendingProviderRemoval, ProviderRemovalError> {
        if self.pending_provider_removal.is_some() {
            return Err(ProviderRemovalError::RemovalInProgress);
        }
        if plan.expected_shell_revision != self.revision {
            return Err(ProviderRemovalError::StalePlan);
        }
        let mut queued_intents = 0;
        let navigation = match plan.fallback {
            Some(fallback) => {
                let before = self.navigator.len();
                self.navigator
                    .commit(fallback)
                    .map_err(ProviderRemovalError::Navigation)?;
                if let Some(fallback_instance) = plan.fallback_instance {
                    let previous = self.active_instance;
                    self.active_instance = fallback_instance;
                    queued_intents += self.intents.purge_instance(previous);
                }
                NavigationPurge {
                    removed_frames: before - self.navigator.len(),
                    active_changed: true,
                }
            }
            None => NavigationPurge {
                removed_frames: 0,
                active_changed: false,
            },
        };
        let (next_composition, composition_delta, composition_result) =
            plan.composition_plan.into_parts();
        for instance in composition_delta
            .leave_live()
            .iter()
            .chain(composition_delta.remove_queued())
        {
            queued_intents += self.intents.purge_instance(instance.token);
        }
        let CompositionPlanResult::Cleanup(composition) = composition_result else {
            unreachable!("provider detach always carries an exact cleanup plan")
        };
        self.composition = next_composition;
        queued_intents += self.intents.purge_provider(plan.owner);
        self.pending_provider_removal = Some(plan.owner);
        self.revision = self.revision.wrapping_add(1);
        Ok(PendingProviderRemoval {
            expected_shell_revision: self.revision,
            owner: plan.owner,
            navigation,
            composition,
            queued_intents,
        })
    }

    pub fn finalize_provider_removal(
        &mut self,
        pending: &PendingProviderRemoval,
        runtime_audit: ProviderRuntimeAudit,
    ) -> Result<ProviderPurge, ProviderRemovalError> {
        if self.pending_provider_removal != Some(pending.owner) {
            return Err(ProviderRemovalError::StalePlan);
        }
        if pending.expected_shell_revision != self.revision {
            return Err(ProviderRemovalError::StalePlan);
        }
        if runtime_audit.owner != pending.owner {
            return Err(ProviderRemovalError::RuntimeAuditMismatch);
        }
        if self.navigator.references_provider(pending.owner)
            || self.composition.references_provider(pending.owner)
            || self.intents.references_provider(pending.owner)
        {
            return Err(ProviderRemovalError::ReferencesRemain);
        }
        let definitions = self.registry.unregister_provider(pending.owner);
        self.pending_provider_removal = None;
        self.revision = self.revision.wrapping_add(1);
        Ok(ProviderPurge {
            definitions,
            navigation: pending.navigation,
            composition: pending.composition,
            queued_intents: pending.queued_intents,
        })
    }

    #[cfg(all(test, not(target_os = "none")))]
    pub fn commit_provider_removal(
        &mut self,
        plan: ProviderRemovalPlan<NAVIGATION, OVERLAYS, MODALS>,
    ) -> Result<ProviderPurge, ProviderRemovalError> {
        let pending = self.commit_provider_detach(plan)?;
        self.finalize_provider_removal(&pending, ProviderRuntimeAudit::verified(pending.owner()))
    }

    pub fn active(&self) -> NavigationFrame {
        self.navigator.active()
    }

    pub fn active_instance(&self) -> SurfaceInstanceToken {
        self.active_instance
    }

    pub fn navigation_len(&self) -> usize {
        self.navigator.len()
    }

    pub fn navigation_frame(&self, index: usize) -> Option<NavigationFrame> {
        self.navigator.frame(index)
    }

    pub fn definition_len(&self) -> usize {
        self.registry.definition_len()
    }

    pub fn provider_len(&self) -> usize {
        self.registry.provider_len()
    }

    pub fn live_overlay_len(&self) -> usize {
        self.composition.live_len()
    }

    pub fn live_overlay(&self, index: usize) -> Option<OverlayInstance> {
        self.composition.live(index)
    }

    pub fn active_modal(&self) -> Option<OverlayInstance> {
        self.composition.active_modal()
    }

    pub fn queued_modal_len(&self) -> usize {
        self.composition.queued_modal_len()
    }

    pub fn queued_modal(&self, index: usize) -> Option<OverlayInstance> {
        self.composition.queued_modal(index)
    }

    pub fn merged_refresh_hint(&self) -> RefreshHint {
        let mut hint = self
            .registry
            .resolve(self.navigator.active().surface)
            .expect("the active frame always has a registered definition")
            .refresh_hint;
        for index in 0..self.composition.live_len() {
            let instance = self
                .composition
                .live(index)
                .expect("index is bounded by the live overlay length");
            let overlay_hint = self
                .registry
                .resolve(instance.token.surface)
                .expect("live overlays always retain registered definitions")
                .refresh_hint;
            hint = hint.max(overlay_hint);
        }
        hint
    }

    pub fn queued_intent_len(&self) -> usize {
        self.intents.len()
    }

    fn validate_overlay(&self, surface: SurfaceRef) -> Result<(), CompositionError> {
        let definition = self
            .registry
            .resolve(surface)
            .map_err(CompositionError::Resolve)?;
        if definition.role != SurfaceRole::Overlay
            || !definition
                .capabilities
                .contains(SurfaceCapabilities::OVERLAY)
        {
            return Err(CompositionError::NotOverlay(surface));
        }
        Ok(())
    }

    fn issue_instance(
        &mut self,
        surface: SurfaceRef,
    ) -> Result<SurfaceInstanceToken, ShellNavigationError> {
        let generation = self.next_instance_generation;
        self.next_instance_generation = generation
            .checked_add(1)
            .ok_or(ShellNavigationError::InstanceGenerationExhausted)?;
        Ok(SurfaceInstanceToken::issued(
            surface,
            InstanceGeneration(generation),
        ))
    }
}

fn validate_surface(surface: SurfaceSpec) -> Result<(), SurfaceContractError> {
    let required = match surface.role {
        SurfaceRole::Ambient => Some(SurfaceCapabilities::AMBIENT),
        SurfaceRole::AppRoot | SurfaceRole::SystemRoot => Some(SurfaceCapabilities::LAUNCHABLE),
        SurfaceRole::Overlay => Some(SurfaceCapabilities::OVERLAY),
        SurfaceRole::Launcher | SurfaceRole::AppChild => None,
    };
    if let Some(required) = required {
        if !surface.capabilities.contains(required) {
            return Err(SurfaceContractError::MissingCapability {
                id: surface.id,
                role: surface.role,
                required,
            });
        }
    }
    Ok(())
}
