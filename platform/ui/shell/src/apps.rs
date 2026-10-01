//! Product-neutral app registry: stable per-app descriptors plus their
//! target-resolved surfaces, without naming any concrete product.
//!
//! A product declares its own app-kind enum (Medinote's, for example) and
//! instantiates [`ProductApps`] with it. Resolution maps an active
//! [`SurfaceRef`] to that kind by checking *every* surface the app owns --
//! not just its launch surface -- so an [`SurfaceRole::AppChild`] pushed on
//! top of an [`SurfaceRole::AppRoot`] still attributes input and timing to
//! the same app.
//!
//! `no_std` compatible and allocation-free: [`ProductApps`] stores
//! registrations in a fixed-capacity [`heapless::Vec`].

use core::ffi::CStr;

use super::catalogue::{CatalogueAvailability, CatalogueEntry, EntryId, GlyphRef};
use super::types::{
    ProviderId, ProviderToken, SurfaceCapabilities, SurfaceId, SurfaceRef, SurfaceSpec,
};

/// Stable metadata for one launchable app, instantiated with the product's
/// own app-kind enum.
pub struct AppDescriptor<K: 'static> {
    /// Product-defined kind tag. States live with each app; this only names
    /// "which one".
    pub kind: K,
    /// Stable catalogue identity, assigned by the app -- never derived from
    /// list position, since persisted settings key on it.
    pub entry_id: EntryId,
    pub provider: ProviderId,
    /// Every surface the app may register, exactly as handed to the shell's
    /// provider registration. Exactly one of these is the launch point.
    pub surfaces: &'static [SurfaceSpec],
    pub label: &'static CStr,
    pub glyph: GlyphRef,
    pub capabilities: SurfaceCapabilities,
    pub default_rank: u8,
    /// Which of `surfaces` the catalogue enters when this app is launched.
    pub launch_surface: SurfaceId,
}

/// An app's descriptor paired with the provider token the shell issued at
/// registration. The intermediate step between a static descriptor and a
/// [`RegisteredApp`]: the target attaches the descriptor to the matching
/// shell-issued owner, whether that provider is dedicated to one app or
/// shared by several apps.
pub struct AppRegistration<K: 'static> {
    pub descriptor: &'static AppDescriptor<K>,
    pub owner: ProviderToken,
}

impl<K: 'static> AppRegistration<K> {
    pub const fn new(descriptor: &'static AppDescriptor<K>, owner: ProviderToken) -> Self {
        Self { descriptor, owner }
    }
}

/// One registered app: its kind, where its launch surface resolved, and
/// which provider owns all of its surfaces.
pub struct RegisteredApp<K: 'static> {
    descriptor: &'static AppDescriptor<K>,
    owner: ProviderToken,
    launch_surface: SurfaceRef,
}

impl<K: Copy + 'static> RegisteredApp<K> {
    const fn new(registration: AppRegistration<K>) -> Self {
        let launch_surface = SurfaceRef {
            owner: registration.owner,
            id: registration.descriptor.launch_surface,
        };
        Self {
            descriptor: registration.descriptor,
            owner: registration.owner,
            launch_surface,
        }
    }

    pub const fn kind(&self) -> K {
        self.descriptor.kind
    }

    pub const fn descriptor(&self) -> &'static AppDescriptor<K> {
        self.descriptor
    }

    pub const fn owner(&self) -> ProviderToken {
        self.owner
    }

    pub const fn launch_surface(&self) -> SurfaceRef {
        self.launch_surface
    }

    /// True for the launch surface and every other surface this app
    /// registered. Owner comparison includes the provider generation, so a
    /// re-registered provider's stale refs never attribute to the new owner.
    pub fn owns(&self, surface: SurfaceRef) -> bool {
        if surface.owner != self.owner {
            return false;
        }
        self.descriptor
            .surfaces
            .iter()
            .any(|spec| spec.id == surface.id)
    }

    /// The catalogue row for this app's launch surface.
    pub fn catalogue_entry(&self) -> CatalogueEntry {
        CatalogueEntry {
            id: self.descriptor.entry_id,
            label: self.descriptor.label,
            glyph: self.descriptor.glyph,
            surface: self.launch_surface,
            capabilities: self.descriptor.capabilities,
            default_rank: self.descriptor.default_rank,
            pin: None,
            availability: CatalogueAvailability::Ready,
        }
    }
}

/// Fixed-capacity registry of a product's resolved apps.
pub struct ProductApps<K: 'static, const N: usize> {
    apps: heapless::Vec<RegisteredApp<K>, N>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductAppsError {
    Capacity,
    ProviderMismatch {
        expected: ProviderId,
        actual: ProviderId,
    },
    DuplicateKind,
    DuplicateSurface(SurfaceRef),
    UnknownLaunchSurface(SurfaceId),
}

impl<K: 'static, const N: usize> ProductApps<K, N> {
    pub const fn new() -> Self {
        Self {
            apps: heapless::Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, RegisteredApp<K>> {
        self.apps.iter()
    }

    pub fn as_slice(&self) -> &[RegisteredApp<K>] {
        self.apps.as_slice()
    }
}

impl<K: 'static, const N: usize> Default for ProductApps<K, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Copy + PartialEq + 'static, const N: usize> ProductApps<K, N> {
    /// Resolve `registration`'s launch surface against its owner and store
    /// it. Rejects descriptors whose launch surface is not one of their own
    /// `surfaces`, so a typo fails at registration rather than resolving to
    /// an unregistered surface later.
    pub fn register(&mut self, registration: AppRegistration<K>) -> Result<(), ProductAppsError> {
        if self.apps.len() == N {
            return Err(ProductAppsError::Capacity);
        }
        if registration.descriptor.provider != registration.owner.id {
            return Err(ProductAppsError::ProviderMismatch {
                expected: registration.descriptor.provider,
                actual: registration.owner.id,
            });
        }
        if !registration
            .descriptor
            .surfaces
            .iter()
            .any(|spec| spec.id == registration.descriptor.launch_surface)
        {
            return Err(ProductAppsError::UnknownLaunchSurface(
                registration.descriptor.launch_surface,
            ));
        }
        if self
            .apps
            .iter()
            .any(|app| app.kind() == registration.descriptor.kind)
        {
            return Err(ProductAppsError::DuplicateKind);
        }
        for spec in registration.descriptor.surfaces {
            let surface = SurfaceRef {
                owner: registration.owner,
                id: spec.id,
            };
            if self.apps.iter().any(|app| app.owns(surface)) {
                return Err(ProductAppsError::DuplicateSurface(surface));
            }
        }
        self.apps
            .push(RegisteredApp::new(registration))
            .map_err(|_| ProductAppsError::Capacity)
    }

    /// Which app, if any, owns `surface` -- launch surface or any other
    /// surface registered under the same provider.
    pub fn app_for_surface(&self, surface: SurfaceRef) -> Option<K> {
        self.apps
            .iter()
            .find(|app| app.owns(surface))
            .map(|app| app.kind())
    }

    /// The resolved launch surface for `kind`, if registered.
    pub fn launch_surface_for(&self, kind: K) -> Option<SurfaceRef> {
        self.apps
            .iter()
            .find(|app| app.kind() == kind)
            .map(|app| app.launch_surface())
    }

    /// Every registered launch surface, in registration order.
    pub fn launch_surfaces(&self) -> impl Iterator<Item = SurfaceRef> + '_ {
        self.apps.iter().map(|app| app.launch_surface())
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use crate::types::{ProviderGeneration, RefreshHint, SurfaceRole};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum TestApp {
        First,
        Second,
    }

    const FIRST_SURFACES: [SurfaceSpec; 2] = [
        SurfaceSpec::new(
            11,
            SurfaceRole::AppRoot,
            SurfaceCapabilities::LAUNCHABLE,
            RefreshHint::Content,
        ),
        SurfaceSpec::new(
            12,
            SurfaceRole::AppChild,
            SurfaceCapabilities::NONE,
            RefreshHint::Content,
        ),
    ];
    const SECOND_SURFACES: [SurfaceSpec; 1] = [SurfaceSpec::new(
        21,
        SurfaceRole::AppRoot,
        SurfaceCapabilities::LAUNCHABLE,
        RefreshHint::Content,
    )];

    const FIRST: AppDescriptor<TestApp> = AppDescriptor {
        kind: TestApp::First,
        entry_id: EntryId::new(9, 1),
        provider: ProviderId(50),
        surfaces: &FIRST_SURFACES,
        label: c"First",
        glyph: GlyphRef(1),
        capabilities: SurfaceCapabilities::LAUNCHABLE,
        default_rank: 0,
        launch_surface: SurfaceId(11),
    };
    const SECOND: AppDescriptor<TestApp> = AppDescriptor {
        kind: TestApp::Second,
        entry_id: EntryId::new(9, 2),
        provider: ProviderId(60),
        surfaces: &SECOND_SURFACES,
        label: c"Second",
        glyph: GlyphRef(2),
        capabilities: SurfaceCapabilities::LAUNCHABLE,
        default_rank: 1,
        launch_surface: SurfaceId(21),
    };

    fn owner(id: u16, generation: u32) -> ProviderToken {
        ProviderToken {
            id: ProviderId(id),
            generation: ProviderGeneration(generation),
        }
    }

    fn registered() -> ProductApps<TestApp, 2> {
        let first_owner = owner(50, 1);
        let second_owner = owner(60, 1);
        let mut apps = ProductApps::new();
        apps.register(AppRegistration::new(&FIRST, first_owner))
            .expect("first registers");
        apps.register(AppRegistration::new(&SECOND, second_owner))
            .expect("second registers");
        apps
    }

    #[test]
    fn owned_child_surface_resolves_to_its_launch_app() {
        let apps = registered();
        let first_owner = owner(50, 1);
        let launch = SurfaceRef {
            owner: first_owner,
            id: SurfaceId(11),
        };
        let child = SurfaceRef {
            owner: first_owner,
            id: SurfaceId(12),
        };
        assert_eq!(apps.app_for_surface(launch), Some(TestApp::First));
        assert_eq!(
            apps.app_for_surface(child),
            Some(TestApp::First),
            "every surface owned by an app resolves, not just its launch surface"
        );
    }

    #[test]
    fn unrelated_and_stale_surfaces_resolve_to_no_app() {
        let apps = registered();
        let stranger = SurfaceRef {
            owner: owner(70, 1),
            id: SurfaceId(11),
        };
        assert_eq!(apps.app_for_surface(stranger), None);
        let stale = SurfaceRef {
            owner: owner(50, 2),
            id: SurfaceId(11),
        };
        assert_eq!(
            apps.app_for_surface(stale),
            None,
            "a re-registered provider generation must not inherit the old owner"
        );
    }

    #[test]
    fn launch_surfaces_and_lookup_stay_with_their_kind() {
        let apps = registered();
        let first_owner = owner(50, 1);
        assert_eq!(
            apps.launch_surface_for(TestApp::First),
            Some(SurfaceRef {
                owner: first_owner,
                id: SurfaceId(11),
            })
        );
        assert_eq!(
            apps.launch_surfaces()
                .collect::<heapless::Vec<_, 2>>()
                .as_slice(),
            &[
                SurfaceRef {
                    owner: first_owner,
                    id: SurfaceId(11),
                },
                SurfaceRef {
                    owner: owner(60, 1),
                    id: SurfaceId(21),
                },
            ]
        );
    }

    #[test]
    fn catalogue_entry_carries_descriptor_metadata_and_launch_surface() {
        let apps = registered();
        let entry = apps.as_slice()[0].catalogue_entry();
        assert_eq!(entry.id, FIRST.entry_id);
        assert_eq!(
            entry.surface,
            apps.launch_surface_for(TestApp::First).unwrap()
        );
        assert_eq!(entry.label, FIRST.label);
        assert_eq!(entry.availability, CatalogueAvailability::Ready);
    }

    #[test]
    fn registration_rejects_overflow_and_unknown_launch_surface() {
        let mut apps = registered();
        assert_eq!(
            apps.register(AppRegistration::new(&FIRST, owner(80, 1))),
            Err(ProductAppsError::Capacity)
        );

        const BAD_SURFACES: [SurfaceSpec; 1] = [SurfaceSpec::new(
            31,
            SurfaceRole::AppRoot,
            SurfaceCapabilities::LAUNCHABLE,
            RefreshHint::Content,
        )];
        const BAD: AppDescriptor<TestApp> = AppDescriptor {
            kind: TestApp::First,
            entry_id: EntryId::new(9, 3),
            provider: ProviderId(80),
            surfaces: &BAD_SURFACES,
            label: c"Bad",
            glyph: GlyphRef(3),
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 2,
            launch_surface: SurfaceId(99),
        };
        let mut fresh: ProductApps<TestApp, 2> = ProductApps::new();
        assert_eq!(
            fresh.register(AppRegistration::new(&BAD, owner(80, 1))),
            Err(ProductAppsError::UnknownLaunchSurface(SurfaceId(99)))
        );
        assert!(fresh.is_empty());
    }

    #[test]
    fn registration_rejects_mismatched_provider_and_ambiguous_membership() {
        let mut apps: ProductApps<TestApp, 2> = ProductApps::new();
        assert_eq!(
            apps.register(AppRegistration::new(&FIRST, owner(60, 1))),
            Err(ProductAppsError::ProviderMismatch {
                expected: ProviderId(50),
                actual: ProviderId(60),
            })
        );

        apps.register(AppRegistration::new(&FIRST, owner(50, 1)))
            .expect("first registers");
        assert_eq!(
            apps.register(AppRegistration::new(&FIRST, owner(50, 1))),
            Err(ProductAppsError::DuplicateKind)
        );

        const OVERLAP: AppDescriptor<TestApp> = AppDescriptor {
            kind: TestApp::Second,
            entry_id: EntryId::new(9, 4),
            provider: ProviderId(50),
            surfaces: &FIRST_SURFACES,
            label: c"Overlap",
            glyph: GlyphRef(4),
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 3,
            launch_surface: SurfaceId(11),
        };
        assert_eq!(
            apps.register(AppRegistration::new(&OVERLAP, owner(50, 1))),
            Err(ProductAppsError::DuplicateSurface(SurfaceRef {
                owner: owner(50, 1),
                id: SurfaceId(11),
            }))
        );
    }

    #[test]
    fn distinct_apps_may_share_one_shell_provider() {
        const SECOND_SHARED: AppDescriptor<TestApp> = AppDescriptor {
            kind: TestApp::Second,
            entry_id: EntryId::new(9, 5),
            provider: ProviderId(50),
            surfaces: &SECOND_SURFACES,
            label: c"Second shared",
            glyph: GlyphRef(5),
            capabilities: SurfaceCapabilities::LAUNCHABLE,
            default_rank: 4,
            launch_surface: SurfaceId(21),
        };
        let shared_owner = owner(50, 1);
        let mut apps: ProductApps<TestApp, 2> = ProductApps::new();
        apps.register(AppRegistration::new(&FIRST, shared_owner))
            .expect("first registers");
        apps.register(AppRegistration::new(&SECOND_SHARED, shared_owner))
            .expect("second registers under the same provider");

        assert_eq!(
            apps.app_for_surface(SurfaceRef {
                owner: shared_owner,
                id: SurfaceId(12),
            }),
            Some(TestApp::First)
        );
        assert_eq!(
            apps.app_for_surface(SurfaceRef {
                owner: shared_owner,
                id: SurfaceId(21),
            }),
            Some(TestApp::Second)
        );
    }
}
