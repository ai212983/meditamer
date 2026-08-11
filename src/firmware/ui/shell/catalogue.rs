use core::{ffi::CStr, mem::size_of};

use heapless::Vec;

use super::types::{SurfaceCapabilities, SurfaceRef};

pub(crate) const CATALOGUE_CAPACITY: usize = 8;

pub(crate) type DefaultCatalogue = CompiledCatalogue<CATALOGUE_CAPACITY>;
pub(crate) type DefaultCatalogueView = CatalogueView<CATALOGUE_CAPACITY>;

pub(crate) const DEFAULT_CATALOGUE_BYTES: usize = size_of::<DefaultCatalogue>();
pub(crate) const DEFAULT_CATALOGUE_VIEW_BYTES: usize = size_of::<DefaultCatalogueView>();

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct EntryId {
    pub(crate) namespace: u16,
    pub(crate) local: u16,
}

impl EntryId {
    pub(crate) const fn new(namespace: u16, local: u16) -> Self {
        Self { namespace, local }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GlyphRef(pub(crate) u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourcePresence {
    BuiltIn,
    LibraryPresent,
    LibraryAbsent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Residency {
    Resident,
    NotResident,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Compatibility {
    Compatible,
    Incompatible,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Health {
    Ready,
    Faulted,
    Corrupt,
    Unverified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogueBadge {
    Ready,
    SourceMissing,
    NotResident,
    Incompatible,
    CompatibilityUnknown,
    Faulted,
    Corrupt,
    Unverified,
    Unregistered,
}

impl CatalogueBadge {
    pub(crate) const fn label(self) -> &'static CStr {
        match self {
            Self::Ready => c"Ready",
            Self::SourceMissing => c"Source missing",
            Self::NotResident => c"Not resident",
            Self::Incompatible => c"Incompatible",
            Self::CompatibilityUnknown => c"Compatibility unknown",
            Self::Faulted => c"Faulted",
            Self::Corrupt => c"Corrupt",
            Self::Unverified => c"Unverified",
            Self::Unregistered => c"Unavailable",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogueAction {
    Enter(SurfaceRef),
    Unavailable(CatalogueBadge),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogueViewKind {
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
pub(crate) struct CatalogueEntry {
    pub(crate) id: EntryId,
    pub(crate) label: &'static CStr,
    pub(crate) glyph: GlyphRef,
    pub(crate) surface: Option<SurfaceRef>,
    pub(crate) capabilities: SurfaceCapabilities,
    pub(crate) default_rank: u8,
    pub(crate) pin: Option<u8>,
    pub(crate) source: SourcePresence,
    pub(crate) residency: Residency,
    pub(crate) compatibility: Compatibility,
    pub(crate) health: Health,
}

impl CatalogueEntry {
    pub(crate) const fn badge(self) -> CatalogueBadge {
        match self.health {
            Health::Faulted => return CatalogueBadge::Faulted,
            Health::Corrupt => return CatalogueBadge::Corrupt,
            Health::Unverified => return CatalogueBadge::Unverified,
            Health::Ready => {}
        }
        match self.compatibility {
            Compatibility::Incompatible => return CatalogueBadge::Incompatible,
            Compatibility::Unknown => return CatalogueBadge::CompatibilityUnknown,
            Compatibility::Compatible => {}
        }
        if self.surface.is_none() {
            return CatalogueBadge::Unregistered;
        }
        match self.residency {
            Residency::NotResident => {
                return if matches!(self.source, SourcePresence::LibraryAbsent) {
                    CatalogueBadge::SourceMissing
                } else {
                    CatalogueBadge::NotResident
                };
            }
            Residency::Resident | Residency::NotApplicable => {}
        }
        CatalogueBadge::Ready
    }

    pub(crate) const fn action(self) -> CatalogueAction {
        match (self.badge(), self.surface) {
            (CatalogueBadge::Ready, Some(surface)) => CatalogueAction::Enter(surface),
            (badge, _) => CatalogueAction::Unavailable(badge),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogueError {
    Empty,
    Capacity,
    DuplicateId(EntryId),
    MissingAmbientFallback(EntryId),
    InvalidAmbientFallback(EntryId),
}

pub(crate) struct CatalogueView<const CAPACITY: usize> {
    entries: Vec<CatalogueEntry, CAPACITY>,
}

impl<const CAPACITY: usize> CatalogueView<CAPACITY> {
    pub(crate) fn entries(&self) -> &[CatalogueEntry] {
        self.entries.as_slice()
    }
}

pub(crate) struct CompiledCatalogue<const CAPACITY: usize> {
    entries: Vec<CatalogueEntry, CAPACITY>,
    ambient_fallback: EntryId,
}

impl<const CAPACITY: usize> CompiledCatalogue<CAPACITY> {
    pub(crate) fn new(
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
            || fallback.source != SourcePresence::BuiltIn
            || fallback.badge() != CatalogueBadge::Ready
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

    pub(crate) fn register(&mut self, entry: CatalogueEntry) -> Result<(), CatalogueError> {
        if self
            .entries
            .iter()
            .any(|registered| registered.id == entry.id)
        {
            return Err(CatalogueError::DuplicateId(entry.id));
        }
        self.entries
            .push(entry)
            .map_err(|_| CatalogueError::Capacity)
    }

    pub(crate) fn view(&self, kind: CatalogueViewKind) -> CatalogueView<CAPACITY> {
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

    pub(crate) fn entry(&self, id: EntryId) -> Option<CatalogueEntry> {
        self.entries.iter().find(|entry| entry.id == id).copied()
    }

    pub(crate) fn entry_is_ready_for(&self, id: EntryId, kind: CatalogueViewKind) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.capabilities.contains(kind.capability()) && entry.badge() == CatalogueBadge::Ready
        })
    }

    pub(crate) fn apply_pins(&mut self, pins: &[EntryId]) {
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

    pub(crate) fn ambient_fallback(&self) -> CatalogueEntry {
        *self
            .entries
            .iter()
            .find(|entry| entry.id == self.ambient_fallback)
            .expect("the constructor validates the fallback entry")
    }

    pub(crate) fn mark_surface_faulted(&mut self, surface: SurfaceRef) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.surface == Some(surface))
        else {
            return false;
        };
        entry.health = Health::Faulted;
        true
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
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
    use crate::shell::types::{
        ProviderGeneration, ProviderId, ProviderToken, SurfaceCapabilities, SurfaceRef,
    };

    const NAMESPACE: u16 = 7;
    const FALLBACK_ID: EntryId = EntryId::new(NAMESPACE, 1);

    #[derive(Clone, Copy)]
    struct EntryAxes {
        source: SourcePresence,
        residency: Residency,
        compatibility: Compatibility,
        health: Health,
    }

    const BUILT_IN_READY: EntryAxes = EntryAxes {
        source: SourcePresence::BuiltIn,
        residency: Residency::NotApplicable,
        compatibility: Compatibility::Compatible,
        health: Health::Ready,
    };
    const LIBRARY_READY: EntryAxes = EntryAxes {
        source: SourcePresence::LibraryPresent,
        residency: Residency::Resident,
        compatibility: Compatibility::Compatible,
        health: Health::Ready,
    };

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
        axes: EntryAxes,
    ) -> CatalogueEntry {
        CatalogueEntry {
            id: EntryId::new(NAMESPACE, local),
            label,
            glyph: GlyphRef(local),
            surface: Some(surface(local)),
            capabilities,
            default_rank,
            pin,
            source: axes.source,
            residency: axes.residency,
            compatibility: axes.compatibility,
            health: axes.health,
        }
    }

    fn fallback() -> CatalogueEntry {
        entry(
            1,
            c"Home",
            SurfaceCapabilities::AMBIENT,
            0,
            None,
            BUILT_IN_READY,
        )
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
            BUILT_IN_READY,
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
    fn state_axes_are_independent_and_resident_entries_survive_source_removal() {
        let base = entry(
            2,
            c"Candidate",
            SurfaceCapabilities::LAUNCHABLE,
            0,
            None,
            LIBRARY_READY,
        );
        assert_eq!(base.badge(), CatalogueBadge::Ready);
        assert_eq!(
            CatalogueEntry {
                source: SourcePresence::LibraryAbsent,
                ..base
            }
            .badge(),
            CatalogueBadge::Ready
        );
        assert_eq!(
            CatalogueEntry {
                residency: Residency::NotResident,
                ..base
            }
            .badge(),
            CatalogueBadge::NotResident
        );
        assert_eq!(
            CatalogueEntry {
                compatibility: Compatibility::Incompatible,
                ..base
            }
            .badge(),
            CatalogueBadge::Incompatible
        );
        assert_eq!(
            CatalogueEntry {
                compatibility: Compatibility::Unknown,
                ..base
            }
            .badge(),
            CatalogueBadge::CompatibilityUnknown
        );
        assert_eq!(
            CatalogueEntry {
                health: Health::Faulted,
                ..base
            }
            .badge(),
            CatalogueBadge::Faulted
        );
    }

    #[test]
    fn ordering_is_pins_then_default_rank_then_id_and_launch_does_not_mutate_it() {
        let entries = [
            fallback(),
            entry(
                5,
                c"Late",
                SurfaceCapabilities::LAUNCHABLE,
                2,
                None,
                BUILT_IN_READY,
            ),
            entry(
                4,
                c"Rank tie",
                SurfaceCapabilities::LAUNCHABLE,
                1,
                None,
                BUILT_IN_READY,
            ),
            entry(
                3,
                c"Pinned second",
                SurfaceCapabilities::LAUNCHABLE,
                9,
                Some(1),
                BUILT_IN_READY,
            ),
            entry(
                2,
                c"Pinned first",
                SurfaceCapabilities::LAUNCHABLE,
                9,
                Some(0),
                BUILT_IN_READY,
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
    fn capacity_duplicate_and_faults_preserve_base_entries_and_fallback() {
        let launchable = entry(
            2,
            c"Diagnostics",
            SurfaceCapabilities::LAUNCHABLE,
            0,
            None,
            BUILT_IN_READY,
        );
        let mut catalogue =
            CompiledCatalogue::<2>::new(&[fallback(), launchable], FALLBACK_ID).unwrap();
        assert_eq!(
            catalogue.register(launchable),
            Err(CatalogueError::DuplicateId(launchable.id))
        );
        assert_eq!(
            catalogue.register(entry(
                3,
                c"Overflow",
                SurfaceCapabilities::LAUNCHABLE,
                1,
                None,
                BUILT_IN_READY,
            )),
            Err(CatalogueError::Capacity)
        );
        assert_eq!(catalogue.len(), 2);
        assert_eq!(catalogue.ambient_fallback().id, FALLBACK_ID);

        assert!(catalogue.mark_surface_faulted(launchable.surface.unwrap()));
        let view = catalogue.view(CatalogueViewKind::Launcher);
        assert_eq!(view.entries().len(), 1);
        assert_eq!(view.entries()[0].badge(), CatalogueBadge::Faulted);
        assert_eq!(catalogue.ambient_fallback().badge(), CatalogueBadge::Ready);
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
            health: Health::Faulted,
            ..fallback()
        };
        assert_eq!(
            CompiledCatalogue::<2>::new(&[faulted], FALLBACK_ID).err(),
            Some(CatalogueError::InvalidAmbientFallback(FALLBACK_ID))
        );
    }
}
