use heapless::Deque;

use super::types::{OwnedShellIntent, ProviderToken, SurfaceInstanceToken};

pub struct CallbackActionQueue<const CAPACITY: usize> {
    pending: Deque<OwnedShellIntent, CAPACITY>,
}

impl<const CAPACITY: usize> CallbackActionQueue<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            pending: Deque::new(),
        }
    }

    pub fn push(&mut self, action: OwnedShellIntent) -> Result<(), OwnedShellIntent> {
        self.pending.push_back(action)
    }

    pub fn pop(&mut self) -> Option<OwnedShellIntent> {
        self.pending.pop_front()
    }

    pub fn purge_instance(&mut self, source: SurfaceInstanceToken) -> usize {
        let before = self.pending.len();
        self.pending.retain(|action| action.source() != source);
        before - self.pending.len()
    }

    pub fn purge_provider(&mut self, owner: ProviderToken) -> usize {
        let before = self.pending.len();
        self.pending
            .retain(|action| !action.references_provider(owner));
        before - self.pending.len()
    }

    pub fn references_provider(&self, owner: ProviderToken) -> bool {
        self.provider_reference_count(owner) != 0
    }

    pub fn provider_reference_count(&self, owner: ProviderToken) -> usize {
        self.pending
            .iter()
            .filter(|action| action.references_provider(owner))
            .count()
    }
}

impl<const CAPACITY: usize> Default for CallbackActionQueue<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
