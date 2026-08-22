use heapless::Vec;

use super::types::{ProviderToken, SurfaceDefinition, SurfaceRef, SurfaceRole};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationFrame {
    pub surface: SurfaceRef,
    pub role: SurfaceRole,
}

impl From<SurfaceDefinition> for NavigationFrame {
    fn from(definition: SurfaceDefinition) -> Self {
        Self {
            surface: definition.surface,
            role: definition.role,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationError {
    Capacity,
    InvalidRole {
        surface: SurfaceRef,
        actual: SurfaceRole,
    },
    InvalidTopology,
    ProviderMismatch {
        root: ProviderToken,
        child: ProviderToken,
    },
    StalePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationOutcome {
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NavigationPurge {
    pub removed_frames: usize,
    pub active_changed: bool,
}

pub struct Navigator<const CAPACITY: usize> {
    frames: Vec<NavigationFrame, CAPACITY>,
    revision: u32,
}

pub struct NavigationPlan<const CAPACITY: usize> {
    expected_revision: u32,
    origin: NavigationFrame,
    next: Vec<NavigationFrame, CAPACITY>,
}

impl<const CAPACITY: usize> NavigationPlan<CAPACITY> {
    pub fn origin(&self) -> NavigationFrame {
        self.origin
    }

    pub fn destination(&self) -> NavigationFrame {
        *self
            .next
            .last()
            .expect("a navigation plan always retains the ambient frame")
    }
}

impl<const CAPACITY: usize> Navigator<CAPACITY> {
    pub fn new(ambient: SurfaceDefinition) -> Result<Self, NavigationError> {
        if ambient.role != SurfaceRole::Ambient {
            return Err(NavigationError::InvalidRole {
                surface: ambient.surface,
                actual: ambient.role,
            });
        }
        let mut frames = Vec::new();
        frames
            .push(ambient.into())
            .map_err(|_| NavigationError::Capacity)?;
        Ok(Self {
            frames,
            revision: 0,
        })
    }

    pub fn prepare_open_launcher(
        &self,
        launcher: SurfaceDefinition,
    ) -> Result<NavigationPlan<CAPACITY>, NavigationError> {
        if launcher.role != SurfaceRole::Launcher {
            return Err(NavigationError::InvalidRole {
                surface: launcher.surface,
                actual: launcher.role,
            });
        }
        let mut next = self.frames.clone();
        next.truncate(1);
        next.push(launcher.into())
            .map_err(|_| NavigationError::Capacity)?;
        Ok(self.plan(next))
    }

    pub fn prepare_launch(
        &self,
        root: SurfaceDefinition,
    ) -> Result<NavigationPlan<CAPACITY>, NavigationError> {
        if !root.role.is_launch_root() {
            return Err(NavigationError::InvalidRole {
                surface: root.surface,
                actual: root.role,
            });
        }
        if self.frames.get(1).map(|frame| frame.role) != Some(SurfaceRole::Launcher) {
            return Err(NavigationError::InvalidTopology);
        }
        let mut next = self.frames.clone();
        next.truncate(2);
        next.push(root.into())
            .map_err(|_| NavigationError::Capacity)?;
        Ok(self.plan(next))
    }

    pub fn prepare_push_child(
        &self,
        child: SurfaceDefinition,
    ) -> Result<NavigationPlan<CAPACITY>, NavigationError> {
        if child.role != SurfaceRole::AppChild {
            return Err(NavigationError::InvalidRole {
                surface: child.surface,
                actual: child.role,
            });
        }
        let root = self.frames.get(2).ok_or(NavigationError::InvalidTopology)?;
        if !root.role.is_launch_root() {
            return Err(NavigationError::InvalidTopology);
        }
        if child.surface.owner != root.surface.owner {
            return Err(NavigationError::ProviderMismatch {
                root: root.surface.owner,
                child: child.surface.owner,
            });
        }
        let mut next = self.frames.clone();
        next.push(child.into())
            .map_err(|_| NavigationError::Capacity)?;
        Ok(self.plan(next))
    }

    pub fn prepare_back(&self) -> NavigationPlan<CAPACITY> {
        let mut next = self.frames.clone();
        if next.len() > 1 {
            next.pop();
        }
        self.plan(next)
    }

    pub fn prepare_home(&self) -> NavigationPlan<CAPACITY> {
        let mut next = self.frames.clone();
        next.truncate(1);
        self.plan(next)
    }

    pub fn owns_ambient(&self, owner: ProviderToken) -> bool {
        self.frames
            .first()
            .is_some_and(|frame| frame.surface.owner == owner)
    }

    pub fn references_provider(&self, owner: ProviderToken) -> bool {
        self.frames.iter().any(|frame| frame.surface.owner == owner)
    }

    pub fn purge_provider(&mut self, owner: ProviderToken) -> NavigationPurge {
        let Some(index) = self
            .frames
            .iter()
            .position(|frame| frame.surface.owner == owner)
        else {
            return NavigationPurge {
                removed_frames: 0,
                active_changed: false,
            };
        };

        let before = self.frames.len();
        self.frames.truncate(index);
        self.revision = self.revision.wrapping_add(1);
        NavigationPurge {
            removed_frames: before - self.frames.len(),
            active_changed: true,
        }
    }

    pub fn active(&self) -> NavigationFrame {
        *self
            .frames
            .last()
            .expect("navigator always retains the ambient frame")
    }

    pub fn frame(&self, index: usize) -> Option<NavigationFrame> {
        self.frames.get(index).copied()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// True when [`Self::len`] is zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn commit(
        &mut self,
        plan: NavigationPlan<CAPACITY>,
    ) -> Result<NavigationOutcome, NavigationError> {
        if plan.expected_revision != self.revision {
            return Err(NavigationError::StalePlan);
        }
        if plan.next == self.frames {
            return Ok(NavigationOutcome::Unchanged);
        }
        self.frames = plan.next;
        self.revision = self.revision.wrapping_add(1);
        Ok(NavigationOutcome::Changed)
    }

    fn plan(&self, next: Vec<NavigationFrame, CAPACITY>) -> NavigationPlan<CAPACITY> {
        NavigationPlan {
            expected_revision: self.revision,
            origin: self.active(),
            next,
        }
    }
}
