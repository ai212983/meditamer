//! Fixed-capacity index of compiled UI surfaces whose providers remain attached for the boot.
//! Runtime provider attachment and surface-instance lifetime belong to the shell registry and
//! lifecycle modules, not this index.

use core::{ffi::CStr, mem::size_of};

use heapless::Vec;

use super::types::{SurfaceCapabilities, SurfaceRef};

pub const CATALOGUE_CAPACITY: usize = 8;

pub type DefaultCatalogue = CompiledCatalogue<CATALOGUE_CAPACITY>;
pub type DefaultCatalogueView = CatalogueView<CATALOGUE_CAPACITY>;

pub const DEFAULT_CATALOGUE_BYTES: usize = size_of::<DefaultCatalogue>();
pub const DEFAULT_CATALOGUE_VIEW_BYTES: usize = size_of::<DefaultCatalogueView>();

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EntryId {
    pub namespace: u16,
    pub local: u16,
}

impl EntryId {
    pub const fn new(namespace: u16, local: u16) -> Self {
        Self { namespace, local }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlyphRef(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueAvailability {
    Ready,
    Faulted,
}

impl CatalogueAvailability {
    pub const fn label(self) -> &'static CStr {
        match self {
            Self::Ready => c"Ready",
            Self::Faulted => c"Faulted",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueAction {
    Enter(SurfaceRef),
    Unavailable(CatalogueAvailability),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueViewKind {
    Launcher,
    AmbientPicker,
    OverlaySettings,
}

impl CatalogueViewKind {
    const fn capability(self) -> SurfaceCapabilities {
        match self {
            Self::Launcher => SurfaceCapabilities::LAUNCHABLE,
            Self::AmbientPicker => SurfaceCapabilities::AMBIENT,
            Self::OverlaySettings => SurfaceCapabilities::OVERLAY,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogueEntry {
    pub id: EntryId,
    pub label: &'static CStr,
    pub glyph: GlyphRef,
    pub surface: SurfaceRef,
    pub capabilities: SurfaceCapabilities,
    pub default_rank: u8,
    pub pin: Option<u8>,
    pub availability: CatalogueAvailability,
}

impl CatalogueEntry {
    pub const fn action(self) -> CatalogueAction {
        match self.availability {
            CatalogueAvailability::Ready => CatalogueAction::Enter(self.surface),
            availability => CatalogueAction::Unavailable(availability),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogueError {
    Empty,
    Capacity,
    DuplicateId(EntryId),
    MissingAmbientFallback(EntryId),
    InvalidAmbientFallback(EntryId),
}

pub struct CatalogueView<const CAPACITY: usize> {
    entries: Vec<CatalogueEntry, CAPACITY>,
}

impl<const CAPACITY: usize> CatalogueView<CAPACITY> {
    pub fn entries(&self) -> &[CatalogueEntry] {
        self.entries.as_slice()
    }
}

pub struct CompiledCatalogue<const CAPACITY: usize> {
    entries: Vec<CatalogueEntry, CAPACITY>,
    ambient_fallback: EntryId,
}

impl<const CAPACITY: usize> CompiledCatalogue<CAPACITY> {
    pub fn new(
        entries: &[CatalogueEntry],
        ambient_fallback: EntryId,
    ) -> Result<Self, CatalogueError> {
        if entries.is_empty() {
            return Err(CatalogueError::Empty);
        }
        if entries.len() > CAPACITY {
            return Err(CatalogueError::Capacity);
        }
        for (index, entry) in entries.iter().enumerate() {
            if entries[..index]
                .iter()
                .any(|registered| registered.id == entry.id)
            {
                return Err(CatalogueError::DuplicateId(entry.id));
            }
        }
        let Some(fallback) = entries.iter().find(|entry| entry.id == ambient_fallback) else {
            return Err(CatalogueError::MissingAmbientFallback(ambient_fallback));
        };
        if !fallback.capabilities.contains(SurfaceCapabilities::AMBIENT)
            || fallback.availability != CatalogueAvailability::Ready
        {
            return Err(CatalogueError::InvalidAmbientFallback(ambient_fallback));
        }

        let mut compiled = Self {
            entries: Vec::new(),
            ambient_fallback,
        };
        for entry in entries {
            compiled
                .entries
                .push(*entry)
                .map_err(|_| CatalogueError::Capacity)?;
        }
        Ok(compiled)
    }

    pub fn view(&self, kind: CatalogueViewKind) -> CatalogueView<CAPACITY> {
        let required = kind.capability();
        let mut entries = Vec::new();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.capabilities.contains(required))
        {
            entries
                .push(*entry)
                .expect("a filtered view cannot exceed its catalogue capacity");
        }
        // Eight entries do not justify linking the generic unstable-sort
        // machinery into firmware. This bounded insertion sort is stable in
        // resource use and keeps equal keys in their compiled order.
        for index in 1..entries.len() {
            let mut cursor = index;
            while cursor > 0 && ordering_key(&entries[cursor]) < ordering_key(&entries[cursor - 1])
            {
                entries.swap(cursor, cursor - 1);
                cursor -= 1;
            }
        }
        CatalogueView { entries }
    }

    pub fn entry(&self, id: EntryId) -> Option<CatalogueEntry> {
        self.entries.iter().find(|entry| entry.id == id).copied()
    }

    pub fn entry_is_ready_for(&self, id: EntryId, kind: CatalogueViewKind) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.capabilities.contains(kind.capability())
                && entry.availability == CatalogueAvailability::Ready
        })
    }

    pub fn apply_pins(&mut self, pins: &[EntryId]) {
        for entry in &mut self.entries {
            entry.pin = None;
        }
        for (position, id) in pins.iter().copied().enumerate() {
            let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
                continue;
            };
            if entry.capabilities.contains(SurfaceCapabilities::LAUNCHABLE) {
                entry.pin = u8::try_from(position).ok();
            }
        }
    }

    pub fn ambient_fallback(&self) -> CatalogueEntry {
        *self
            .entries
            .iter()
            .find(|entry| entry.id == self.ambient_fallback)
            .expect("the constructor validates the fallback entry")
    }

    pub fn mark_surface_faulted(&mut self, surface: SurfaceRef) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.surface == surface)
        else {
            return false;
        };
        entry.availability = CatalogueAvailability::Faulted;
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when [`Self::len`] is zero.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

const fn ordering_key(entry: &CatalogueEntry) -> (u8, u8, u8, EntryId) {
    match entry.pin {
        Some(position) => (0, position, 0, entry.id),
        None => (1, 0, entry.default_rank, entry.id),
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::types::{
        ProviderGeneration, ProviderId, ProviderToken, SurfaceCapabilities, SurfaceRef,
    };

    const NAMESPACE: u16 = 7;
    const FALLBACK_ID: EntryId = EntryId::new(NAMESPACE, 1);

    fn surface(id: u16) -> SurfaceRef {
        SurfaceRef::new(
            ProviderToken {
                id: ProviderId(1),
                generation: ProviderGeneration(1),
            },
            id,
        )
    }

    fn entry(
        local: u16,
        label: &'static CStr,
        capabilities: SurfaceCapabilities,
        default_rank: u8,
        pin: Option<u8>,
    ) -> CatalogueEntry {
        CatalogueEntry {
            id: EntryId::new(NAMESPACE, local),
            label,
            glyph: GlyphRef(local),
            surface: surface(local),
            capabilities,
            default_rank,
            pin,
            availability: CatalogueAvailability::Ready,
        }
    }

    fn fallback() -> CatalogueEntry {
        entry(1, c"Home", SurfaceCapabilities::AMBIENT, 0, None)
    }

    #[test]
    fn views_filter_capabilities_under_one_stable_entry_id() {
        let shared = entry(
            2,
            c"Shared",
            SurfaceCapabilities::LAUNCHABLE
                .union(SurfaceCapabilities::AMBIENT)
                .union(SurfaceCapabilities::OVERLAY),
            2,
            None,
        );
        let catalogue = CompiledCatalogue::<4>::new(&[fallback(), shared], FALLBACK_ID).unwrap();

        for kind in [
            CatalogueViewKind::Launcher,
            CatalogueViewKind::AmbientPicker,
            CatalogueViewKind::OverlaySettings,
        ] {
            assert!(catalogue
                .view(kind)
                .entries()
                .iter()
                .any(|candidate| candidate.id == shared.id));
        }
    }

    #[test]
    fn runtime_fault_disables_entry_without_changing_its_identity() {
        let ready = entry(2, c"Candidate", SurfaceCapabilities::LAUNCHABLE, 0, None);
        let faulted = CatalogueEntry {
            availability: CatalogueAvailability::Faulted,
            ..ready
        };

        assert_eq!(ready.availability, CatalogueAvailability::Ready);
        assert_eq!(ready.action(), CatalogueAction::Enter(ready.surface));
        assert_eq!(faulted.id, ready.id);
        assert_eq!(faulted.availability, CatalogueAvailability::Faulted);
        assert_eq!(
            faulted.action(),
            CatalogueAction::Unavailable(CatalogueAvailability::Faulted)
        );
    }

    #[test]
    fn ordering_is_pins_then_default_rank_then_id_and_launch_does_not_mutate_it() {
        let entries = [
            fallback(),
            entry(5, c"Late", SurfaceCapabilities::LAUNCHABLE, 2, None),
            entry(4, c"Rank tie", SurfaceCapabilities::LAUNCHABLE, 1, None),
            entry(
                3,
                c"Pinned second",
                SurfaceCapabilities::LAUNCHABLE,
                9,
                Some(1),
            ),
            entry(
                2,
                c"Pinned first",
                SurfaceCapabilities::LAUNCHABLE,
                9,
                Some(0),
            ),
        ];
        let catalogue = CompiledCatalogue::<6>::new(&entries, FALLBACK_ID).unwrap();
        let before = catalogue.view(CatalogueViewKind::Launcher);
        let selected = before.entries()[2].action();
        assert!(matches!(selected, CatalogueAction::Enter(_)));
        let after = catalogue.view(CatalogueViewKind::Launcher);

        let expected = [
            EntryId::new(NAMESPACE, 2),
            EntryId::new(NAMESPACE, 3),
            EntryId::new(NAMESPACE, 4),
            EntryId::new(NAMESPACE, 5),
        ];
        assert_eq!(
            before
                .entries()
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_, 6>>()
                .as_slice(),
            expected
        );
        assert_eq!(before.entries(), after.entries());
    }

    #[test]
    fn construction_limits_and_faults_preserve_base_entries_and_fallback() {
        let launchable = entry(2, c"Diagnostics", SurfaceCapabilities::LAUNCHABLE, 0, None);
        let mut catalogue =
            CompiledCatalogue::<2>::new(&[fallback(), launchable], FALLBACK_ID).unwrap();
        assert_eq!(
            CompiledCatalogue::<2>::new(&[fallback(), launchable, launchable], FALLBACK_ID).err(),
            Some(CatalogueError::Capacity)
        );
        assert_eq!(
            CompiledCatalogue::<3>::new(&[fallback(), launchable, launchable], FALLBACK_ID).err(),
            Some(CatalogueError::DuplicateId(launchable.id))
        );
        assert_eq!(catalogue.len(), 2);
        assert_eq!(catalogue.ambient_fallback().id, FALLBACK_ID);

        assert!(catalogue.mark_surface_faulted(launchable.surface));
        let view = catalogue.view(CatalogueViewKind::Launcher);
        assert_eq!(view.entries().len(), 1);
        assert_eq!(
            view.entries()[0].availability,
            CatalogueAvailability::Faulted
        );
        assert_eq!(
            catalogue.ambient_fallback().availability,
            CatalogueAvailability::Ready
        );
    }

    #[test]
    fn invalid_fallbacks_fail_closed() {
        assert_eq!(
            CompiledCatalogue::<2>::new(&[fallback()], EntryId::new(NAMESPACE, 99)).err(),
            Some(CatalogueError::MissingAmbientFallback(EntryId::new(
                NAMESPACE, 99
            )))
        );
        let faulted = CatalogueEntry {
            availability: CatalogueAvailability::Faulted,
            ..fallback()
        };
        assert_eq!(
            CompiledCatalogue::<2>::new(&[faulted], FALLBACK_ID).err(),
            Some(CatalogueError::InvalidAmbientFallback(FALLBACK_ID))
        );
    }
}
