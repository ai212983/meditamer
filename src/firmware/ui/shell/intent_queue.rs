use heapless::Deque;

use super::types::{OwnedNavIntent, ProviderToken, SurfaceInstanceToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntentQueueError {
    Capacity,
    InvalidOwner,
    ProviderRemovalInProgress,
    StaleInstance,
}

pub(crate) struct IntentQueue<const CAPACITY: usize> {
    pending: Deque<OwnedNavIntent, CAPACITY>,
}

impl<const CAPACITY: usize> IntentQueue<CAPACITY> {
    pub(crate) const fn new() -> Self {
        Self {
            pending: Deque::new(),
        }
    }

    pub(crate) fn push(&mut self, intent: OwnedNavIntent) -> Result<(), IntentQueueError> {
        self.pending
            .push_back(intent)
            .map_err(|_| IntentQueueError::Capacity)
    }

    pub(crate) fn pop(&mut self) -> Option<OwnedNavIntent> {
        self.pending.pop_front()
    }

    pub(crate) fn purge_provider(&mut self, owner: ProviderToken) -> usize {
        let before = self.pending.len();
        self.pending.retain(|intent| {
            intent.source.surface.owner != owner && !intent.intent.references_provider(owner)
        });
        before - self.pending.len()
    }

    pub(crate) fn purge_instance(&mut self, source: SurfaceInstanceToken) -> usize {
        let before = self.pending.len();
        self.pending.retain(|intent| intent.source != source);
        before - self.pending.len()
    }

    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn references_provider(&self, owner: ProviderToken) -> bool {
        self.pending.iter().any(|intent| {
            intent.source.surface.owner == owner || intent.intent.references_provider(owner)
        })
    }
}
