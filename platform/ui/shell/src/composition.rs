use heapless::Vec;

use super::types::{
    OverlayAdmission, OverlayDismissal, OverlayInput, OverlayInstance, OverlayLifetime,
    ProviderToken, SurfaceInstanceToken,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionReferenceError {
    DuplicateInstance(SurfaceInstanceToken),
    LiveOverlayCapacity,
    ModalQueueCapacity,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompositionPurge {
    pub live_overlays: usize,
    pub queued_modals: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionDelta<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize> {
    enter_live: Vec<OverlayInstance, LIVE_CAPACITY>,
    leave_live: Vec<OverlayInstance, LIVE_CAPACITY>,
    remove_queued: Vec<OverlayInstance, MODAL_CAPACITY>,
}

impl<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY>
{
    const fn new() -> Self {
        Self {
            enter_live: Vec::new(),
            leave_live: Vec::new(),
            remove_queued: Vec::new(),
        }
    }

    pub fn enter_live(&self) -> &[OverlayInstance] {
        self.enter_live.as_slice()
    }

    pub fn leave_live(&self) -> &[OverlayInstance] {
        self.leave_live.as_slice()
    }

    pub fn remove_queued(&self) -> &[OverlayInstance] {
        self.remove_queued.as_slice()
    }

    pub fn is_empty(&self) -> bool {
        self.enter_live.is_empty() && self.leave_live.is_empty() && self.remove_queued.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionPlanResult {
    Admission(OverlayAdmission),
    Removal(OverlayDismissal),
    Cleanup(CompositionPurge),
}

pub struct CompositionPlan<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize> {
    next: CompositionReferences<LIVE_CAPACITY, MODAL_CAPACITY>,
    delta: CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY>,
    result: CompositionPlanResult,
}

impl<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>
{
    pub fn delta(&self) -> &CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY> {
        &self.delta
    }

    pub fn result(&self) -> CompositionPlanResult {
        self.result
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        CompositionReferences<LIVE_CAPACITY, MODAL_CAPACITY>,
        CompositionDelta<LIVE_CAPACITY, MODAL_CAPACITY>,
        CompositionPlanResult,
    ) {
        (self.next, self.delta, self.result)
    }
}

#[derive(Clone)]
pub struct CompositionReferences<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize> {
    live_overlays: Vec<OverlayInstance, LIVE_CAPACITY>,
    modal_queue: Vec<OverlayInstance, MODAL_CAPACITY>,
}

impl<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize>
    CompositionReferences<LIVE_CAPACITY, MODAL_CAPACITY>
{
    pub const fn new() -> Self {
        Self {
            live_overlays: Vec::new(),
            modal_queue: Vec::new(),
        }
    }

    pub fn plan_admission(
        &self,
        instance: OverlayInstance,
    ) -> Result<CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>, CompositionReferenceError> {
        let mut next = self.clone();
        let admission = next.admit(instance)?;
        Ok(self.plan_diff(next, CompositionPlanResult::Admission(admission)))
    }

    pub fn plan_removal(
        &self,
        token: SurfaceInstanceToken,
    ) -> Option<CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY>> {
        let expected = self.preview_remove(token)?;
        let mut next = self.clone();
        let removal = next
            .remove(token)
            .expect("the cloned composition contains the planned instance");
        debug_assert_eq!(removal, expected);
        let mut delta = CompositionDelta::new();
        if removal.removed_was_live {
            delta
                .leave_live
                .push(removal.removed)
                .expect("one removal fits the declared live capacity");
        } else {
            delta
                .remove_queued
                .push(removal.removed)
                .expect("one removal fits the declared modal capacity");
        }
        if let Some(promoted) = removal.promoted {
            delta
                .enter_live
                .push(promoted)
                .expect("one promotion fits the declared live capacity");
        }
        Some(CompositionPlan {
            next,
            delta,
            result: CompositionPlanResult::Removal(removal),
        })
    }

    pub fn plan_drop_transient(&self) -> CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY> {
        let mut next = self.clone();
        let result = next.drop_transient();
        self.plan_exact_delta(next, CompositionPlanResult::Cleanup(result), |instance| {
            instance.lifetime == OverlayLifetime::Transient
        })
    }

    pub fn plan_provider_detach(
        &self,
        owner: ProviderToken,
        drop_transients: bool,
    ) -> CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY> {
        let mut next = self.clone();
        if drop_transients {
            let _ = next.drop_transient();
        }
        let _ = next.purge_provider(owner);
        let result = CompositionPurge {
            live_overlays: self
                .live_overlays
                .iter()
                .filter(|instance| {
                    drop_transients && instance.lifetime == OverlayLifetime::Transient
                        || instance.token.surface.owner == owner
                        || instance.request_owner == owner
                })
                .count(),
            queued_modals: self
                .modal_queue
                .iter()
                .filter(|instance| {
                    drop_transients && instance.lifetime == OverlayLifetime::Transient
                        || instance.token.surface.owner == owner
                        || instance.request_owner == owner
                })
                .count(),
        };
        self.plan_exact_delta(next, CompositionPlanResult::Cleanup(result), |instance| {
            drop_transients && instance.lifetime == OverlayLifetime::Transient
                || instance.token.surface.owner == owner
                || instance.request_owner == owner
        })
    }

    pub fn admit(
        &mut self,
        instance: OverlayInstance,
    ) -> Result<OverlayAdmission, CompositionReferenceError> {
        let admission = self.preview_admission(instance)?;
        match admission {
            OverlayAdmission::Active(instance) => {
                if instance.input == OverlayInput::Modal {
                    if let Some(index) = self.active_modal_index() {
                        let preempted = self.live_overlays.remove(index);
                        debug_assert_eq!(preempted.band, super::types::OverlayBand::Provider);
                        debug_assert_eq!(instance.band, super::types::OverlayBand::BaseSystem);
                    }
                }
                self.insert_live(instance)?;
            }
            OverlayAdmission::Queued(instance) => {
                if self.modal_queue.len() == MODAL_CAPACITY {
                    debug_assert_eq!(instance.band, super::types::OverlayBand::BaseSystem);
                    let provider = self
                        .modal_queue
                        .iter()
                        .rposition(|queued| queued.band == super::types::OverlayBand::Provider)
                        .expect("base admission at capacity reserves a provider queue slot");
                    self.modal_queue.remove(provider);
                }
                self.enqueue_modal(instance)?;
            }
        }
        Ok(admission)
    }

    pub fn preview_admission(
        &self,
        instance: OverlayInstance,
    ) -> Result<OverlayAdmission, CompositionReferenceError> {
        if self.contains(instance.token) {
            return Err(CompositionReferenceError::DuplicateInstance(instance.token));
        }
        if instance.input == OverlayInput::Modal {
            if let Some(active) = self.active_modal() {
                if instance.band == super::types::OverlayBand::BaseSystem
                    && active.band == super::types::OverlayBand::Provider
                {
                    return Ok(OverlayAdmission::Active(instance));
                }
                if self.modal_queue.len() == MODAL_CAPACITY
                    && (instance.band != super::types::OverlayBand::BaseSystem
                        || !self
                            .modal_queue
                            .iter()
                            .any(|queued| queued.band == super::types::OverlayBand::Provider))
                {
                    return Err(CompositionReferenceError::ModalQueueCapacity);
                }
                return Ok(OverlayAdmission::Queued(instance));
            }
        }
        if self.live_overlays.len() == LIVE_CAPACITY {
            return Err(CompositionReferenceError::LiveOverlayCapacity);
        }
        Ok(OverlayAdmission::Active(instance))
    }

    pub fn remove(&mut self, token: SurfaceInstanceToken) -> Option<OverlayDismissal> {
        let expected = self.preview_remove(token)?;
        if let Some(index) = self
            .live_overlays
            .iter()
            .position(|instance| instance.token == token)
        {
            let removed = self.live_overlays.remove(index);
            let promoted = (removed.input == OverlayInput::Modal)
                .then(|| self.promote_if_modal_vacant())
                .flatten();
            let dismissal = OverlayDismissal {
                removed,
                removed_was_live: true,
                promoted,
            };
            debug_assert_eq!(dismissal, expected);
            return Some(dismissal);
        }
        let index = self
            .modal_queue
            .iter()
            .position(|instance| instance.token == token)?;
        let dismissal = OverlayDismissal {
            removed: self.modal_queue.remove(index),
            removed_was_live: false,
            promoted: None,
        };
        debug_assert_eq!(dismissal, expected);
        Some(dismissal)
    }

    pub fn preview_remove(&self, token: SurfaceInstanceToken) -> Option<OverlayDismissal> {
        if let Some(removed) = self
            .live_overlays
            .iter()
            .find(|instance| instance.token == token)
            .copied()
        {
            let promoted = (removed.input == OverlayInput::Modal)
                .then(|| self.modal_queue.first().copied())
                .flatten();
            return Some(OverlayDismissal {
                removed,
                removed_was_live: true,
                promoted,
            });
        }
        let removed = self
            .modal_queue
            .iter()
            .find(|instance| instance.token == token)
            .copied()?;
        Some(OverlayDismissal {
            removed,
            removed_was_live: false,
            promoted: None,
        })
    }

    pub fn dismiss_active_modal(&mut self) -> Option<OverlayDismissal> {
        let index = self
            .live_overlays
            .iter()
            .position(|instance| instance.input == OverlayInput::Modal)?;
        let removed = self.live_overlays.remove(index);
        let promoted = if self.modal_queue.is_empty() {
            None
        } else {
            let promoted = self.modal_queue.remove(0);
            self.insert_live(promoted)
                .expect("removing the active modal reserves one live slot");
            Some(promoted)
        };
        Some(OverlayDismissal {
            removed,
            removed_was_live: true,
            promoted,
        })
    }

    pub fn drop_transient(&mut self) -> CompositionPurge {
        let purge = self.purge_matching(|instance| instance.lifetime == OverlayLifetime::Transient);
        let _ = self.promote_if_modal_vacant();
        purge
    }

    pub fn purge_provider(&mut self, owner: ProviderToken) -> CompositionPurge {
        let purge = self.purge_matching(|instance| {
            instance.token.surface.owner == owner || instance.request_owner == owner
        });
        let _ = self.promote_if_modal_vacant();
        purge
    }

    pub fn active_modal(&self) -> Option<OverlayInstance> {
        self.live_overlays
            .iter()
            .find(|instance| instance.input == OverlayInput::Modal)
            .copied()
    }

    pub fn live(&self, index: usize) -> Option<OverlayInstance> {
        self.live_overlays.get(index).copied()
    }

    pub fn queued_modal(&self, index: usize) -> Option<OverlayInstance> {
        self.modal_queue.get(index).copied()
    }

    pub fn first_transient(&self) -> Option<OverlayInstance> {
        self.live_overlays
            .iter()
            .chain(self.modal_queue.iter())
            .find(|instance| instance.lifetime == OverlayLifetime::Transient)
            .copied()
    }

    pub fn live_len(&self) -> usize {
        self.live_overlays.len()
    }

    pub fn queued_modal_len(&self) -> usize {
        self.modal_queue.len()
    }

    pub fn references_provider(&self, owner: ProviderToken) -> bool {
        self.live_overlays
            .iter()
            .chain(self.modal_queue.iter())
            .any(|instance| {
                instance.token.surface.owner == owner || instance.request_owner == owner
            })
    }

    fn contains(&self, token: SurfaceInstanceToken) -> bool {
        self.live_overlays
            .iter()
            .chain(self.modal_queue.iter())
            .any(|instance| instance.token == token)
    }

    fn contains_live(&self, token: SurfaceInstanceToken) -> bool {
        self.live_overlays
            .iter()
            .any(|instance| instance.token == token)
    }

    fn plan_diff(
        &self,
        next: Self,
        result: CompositionPlanResult,
    ) -> CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY> {
        let mut delta = CompositionDelta::new();
        for instance in self.live_overlays.iter().copied() {
            if !next.contains_live(instance.token) {
                delta
                    .leave_live
                    .push(instance)
                    .expect("removed live entries fit their source capacity");
            }
        }
        for instance in self.modal_queue.iter().copied() {
            if !next.contains(instance.token) {
                delta
                    .remove_queued
                    .push(instance)
                    .expect("removed queued entries fit their source capacity");
            }
        }
        for instance in next.live_overlays.iter().copied() {
            if !self.contains_live(instance.token) {
                delta
                    .enter_live
                    .push(instance)
                    .expect("new live entries fit the declared capacity");
            }
        }
        CompositionPlan {
            next,
            delta,
            result,
        }
    }

    fn plan_exact_delta(
        &self,
        next: Self,
        result: CompositionPlanResult,
        mut removed: impl FnMut(OverlayInstance) -> bool,
    ) -> CompositionPlan<LIVE_CAPACITY, MODAL_CAPACITY> {
        let mut delta = CompositionDelta::new();
        for instance in self
            .live_overlays
            .iter()
            .copied()
            .filter(|instance| removed(*instance))
        {
            delta
                .leave_live
                .push(instance)
                .expect("removed live entries fit their source capacity");
        }
        for instance in self
            .modal_queue
            .iter()
            .copied()
            .filter(|instance| removed(*instance))
        {
            delta
                .remove_queued
                .push(instance)
                .expect("removed queued entries fit their source capacity");
        }
        for instance in next.live_overlays.iter().copied() {
            if !self.contains_live(instance.token) {
                delta
                    .enter_live
                    .push(instance)
                    .expect("promoted entries fit the declared live capacity");
            }
        }
        CompositionPlan {
            next,
            delta,
            result,
        }
    }

    fn insert_live(&mut self, instance: OverlayInstance) -> Result<(), CompositionReferenceError> {
        self.live_overlays
            .push(instance)
            .map_err(|_| CompositionReferenceError::LiveOverlayCapacity)?;
        let mut index = self.live_overlays.len() - 1;
        while index > 0
            && ordering_key(self.live_overlays[index - 1]) > ordering_key(self.live_overlays[index])
        {
            self.live_overlays.swap(index - 1, index);
            index -= 1;
        }
        Ok(())
    }

    fn enqueue_modal(
        &mut self,
        instance: OverlayInstance,
    ) -> Result<(), CompositionReferenceError> {
        self.modal_queue
            .push(instance)
            .map_err(|_| CompositionReferenceError::ModalQueueCapacity)?;
        let mut index = self.modal_queue.len() - 1;
        while index > 0
            && modal_priority(self.modal_queue[index - 1]) > modal_priority(self.modal_queue[index])
        {
            self.modal_queue.swap(index - 1, index);
            index -= 1;
        }
        Ok(())
    }

    fn active_modal_index(&self) -> Option<usize> {
        self.live_overlays
            .iter()
            .position(|instance| instance.input == OverlayInput::Modal)
    }

    fn promote_if_modal_vacant(&mut self) -> Option<OverlayInstance> {
        if self.active_modal().is_some() || self.modal_queue.is_empty() {
            return None;
        }
        let promoted = self.modal_queue.remove(0);
        self.insert_live(promoted)
            .expect("removing or purging an active modal reserves one live slot");
        Some(promoted)
    }

    fn purge_matching(
        &mut self,
        mut matches: impl FnMut(OverlayInstance) -> bool,
    ) -> CompositionPurge {
        let live_overlays = purge_matching(&mut self.live_overlays, &mut matches);
        let queued_modals = purge_matching(&mut self.modal_queue, matches);
        CompositionPurge {
            live_overlays,
            queued_modals,
        }
    }
}

fn ordering_key(instance: OverlayInstance) -> (super::types::OverlayBand, u8, u32) {
    (instance.band, instance.rank, instance.token.generation.0)
}

const fn modal_priority(instance: OverlayInstance) -> u8 {
    match instance.band {
        super::types::OverlayBand::BaseSystem => 0,
        super::types::OverlayBand::Provider => 1,
    }
}

fn purge_matching<T, const CAPACITY: usize>(
    values: &mut Vec<T, CAPACITY>,
    mut matches: impl FnMut(T) -> bool,
) -> usize
where
    T: Copy,
{
    let mut index = 0;
    let mut removed = 0;
    while index < values.len() {
        if matches(values[index]) {
            values.remove(index);
            removed += 1;
        } else {
            index += 1;
        }
    }
    removed
}

impl<const LIVE_CAPACITY: usize, const MODAL_CAPACITY: usize> Default
    for CompositionReferences<LIVE_CAPACITY, MODAL_CAPACITY>
{
    fn default() -> Self {
        Self::new()
    }
}
