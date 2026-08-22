use heapless::Deque;

use super::types::{OwnedNavIntent, ProviderToken, SurfaceInstanceToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentQueueError {
    Capacity,
    InvalidOwner,
    ProviderRemovalInProgress,
    StaleInstance,
}

pub struct IntentQueue<const CAPACITY: usize> {
    pending: Deque<OwnedNavIntent, CAPACITY>,
}

impl<const CAPACITY: usize> IntentQueue<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            pending: Deque::new(),
        }
    }

    pub fn push(&mut self, intent: OwnedNavIntent) -> Result<(), IntentQueueError> {
        self.pending
            .push_back(intent)
            .map_err(|_| IntentQueueError::Capacity)
    }

    pub fn pop(&mut self) -> Option<OwnedNavIntent> {
        self.pending.pop_front()
    }

    pub fn purge_provider(&mut self, owner: ProviderToken) -> usize {
        let before = self.pending.len();
        self.pending.retain(|intent| {
            intent.source.surface.owner != owner && !intent.intent.references_provider(owner)
        });
        before - self.pending.len()
    }

    pub fn purge_instance(&mut self, source: SurfaceInstanceToken) -> usize {
        let before = self.pending.len();
        self.pending.retain(|intent| intent.source != source);
        before - self.pending.len()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// True when [`Self::len`] is zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn references_provider(&self, owner: ProviderToken) -> bool {
        self.pending.iter().any(|intent| {
            intent.source.surface.owner == owner || intent.intent.references_provider(owner)
        })
    }
}

impl<const CAPACITY: usize> Default for IntentQueue<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
