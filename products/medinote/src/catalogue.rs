//! Medinote's product catalogue: the Home ambient surface and launchable apps.
//!
//! Reuses `platform/ui/shell`'s generic `CompiledCatalogue` directly rather than
//! adding a product-local catalogue type (the scope decision recorded in the
//! [hourglass app ledger](../../../docs/archive/features/medinote-hourglass-app-ledger.md)).
//! Catalogue construction happens at runtime, in
//! `targets/medinote-waveshare`, once the shell's `SurfaceRegistry` has
//! issued the real `SurfaceRef`s for every registered provider -- this
//! module only names Home's stable ids and assembles entries from whatever
//! [`crate::apps::descriptor::AppDescriptor`]s the target resolved: adding
//! an app is a descriptor change, not an edit here.

use shell::catalogue::{
    CatalogueAvailability, CatalogueEntry, CatalogueError, CompiledCatalogue, EntryId, GlyphRef,
};
use shell::types::{ProviderId, SurfaceCapabilities, SurfaceId, SurfaceRef};

use crate::apps::descriptor::MedinoteApps;

/// Medinote's product-local catalogue namespace. Entry ids are persisted and
/// compared only within this product's catalogue; this is not a global
/// cross-product namespace.
pub const NAMESPACE: u16 = 1;

pub const HOME_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 1);
pub const BASE_PROVIDER_ID: ProviderId = ProviderId(1);
pub const HOME_SURFACE_ID: SurfaceId = SurfaceId(1);
pub const LAUNCHER_SURFACE_ID: SurfaceId = SurfaceId(2);
pub const CHEERTOK_STATUS_SURFACE_ID: SurfaceId = SurfaceId(4);
pub const SETTINGS_SURFACE_ID: SurfaceId = SurfaceId(5);

/// Sized for a couple more apps before the shell's own `CATALOGUE_CAPACITY`
/// default would be worth reconsidering.
pub type MedinoteCatalogue = CompiledCatalogue<4>;

/// Build the catalogue from Home (the ambient fallback -- not itself an
/// "app", so it has no descriptor of its own) plus every registered app in
/// `apps`, in registration order. Each entry comes from the app's own
/// [`RegisteredApp::catalogue_entry`](shell::apps::RegisteredApp::catalogue_entry),
/// so entry identity still comes from each descriptor's stable `entry_id`,
/// never from list position -- registration order only affects
/// catalogue ranking/display order for equal ranks.
pub fn build(
    home_surface: SurfaceRef,
    apps: &MedinoteApps,
) -> Result<MedinoteCatalogue, CatalogueError> {
    let mut entries: heapless::Vec<CatalogueEntry, 4> = heapless::Vec::new();
    entries
        .push(CatalogueEntry {
            id: HOME_ENTRY_ID,
            label: c"Home",
            glyph: GlyphRef(1),
            surface: home_surface,
            capabilities: SurfaceCapabilities::AMBIENT,
            default_rank: 0,
            pin: None,
            availability: CatalogueAvailability::Ready,
        })
        .map_err(|_| CatalogueError::Capacity)?;
    for app in apps.iter() {
        entries
            .push(app.catalogue_entry())
            .map_err(|_| CatalogueError::Capacity)?;
    }
    MedinoteCatalogue::new(&entries, HOME_ENTRY_ID)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::counter;
    use crate::apps::descriptor::MedinoteRegistration;
    use crate::apps::hourglass::descriptor as hourglass;
    use shell::catalogue::CatalogueViewKind;
    use shell::registry::SurfaceRegistry;
    use shell::types::{ProviderGeneration, ProviderToken, RefreshHint, SurfaceRole, SurfaceSpec};

    /// Stands in for what a real target's `SurfaceRegistry::register_provider`
    /// hands back, without needing a registry roundtrip per app.
    fn token(provider: ProviderId) -> ProviderToken {
        ProviderToken {
            id: provider,
            generation: ProviderGeneration(1),
        }
    }

    #[test]
    fn home_is_ambient_and_hourglass_is_only_launchable() {
        let mut registry: SurfaceRegistry<2, 4> = SurfaceRegistry::new();
        let base = registry
            .register_provider(
                BASE_PROVIDER_ID,
                &[SurfaceSpec::new(
                    HOME_SURFACE_ID.0,
                    SurfaceRole::Ambient,
                    SurfaceCapabilities::AMBIENT,
                    RefreshHint::Content,
                )],
            )
            .expect("base provider registers");
        let token = registry
            .register_provider(hourglass::PROVIDER_ID, hourglass::DESCRIPTOR.surfaces)
            .expect("provider registers");
        let surface = shell::types::SurfaceRef {
            owner: token,
            id: hourglass::DESCRIPTOR.launch_surface,
        };
        registry.resolve(surface).expect("surface resolves");

        let mut apps = MedinoteApps::new();
        apps.register(MedinoteRegistration::new(&hourglass::DESCRIPTOR, token))
            .expect("hourglass registers");
        let home = shell::types::SurfaceRef {
            owner: base,
            id: HOME_SURFACE_ID,
        };
        let catalogue = build(home, &apps).expect("catalogue builds");
        assert_eq!(catalogue.len(), 2);
        assert_eq!(catalogue.ambient_fallback().id, HOME_ENTRY_ID);
        assert_eq!(
            catalogue.view(CatalogueViewKind::Launcher).entries().len(),
            1
        );
        assert_eq!(
            catalogue.view(CatalogueViewKind::Launcher).entries()[0].id,
            hourglass::ENTRY_ID
        );
        assert_eq!(
            catalogue
                .view(CatalogueViewKind::AmbientPicker)
                .entries()
                .len(),
            1
        );
        assert_eq!(
            catalogue.view(CatalogueViewKind::AmbientPicker).entries()[0].id,
            HOME_ENTRY_ID
        );
    }

    #[test]
    fn every_product_app_fits_the_catalogue_without_truncation() {
        let home = shell::types::SurfaceRef {
            owner: token(BASE_PROVIDER_ID),
            id: HOME_SURFACE_ID,
        };
        let mut apps = MedinoteApps::new();
        apps.register(MedinoteRegistration::new(
            &hourglass::DESCRIPTOR,
            token(hourglass::PROVIDER_ID),
        ))
        .expect("hourglass registers");
        apps.register(MedinoteRegistration::new(
            &counter::descriptor::DESCRIPTOR,
            token(counter::descriptor::PROVIDER_ID),
        ))
        .expect("counter registers");
        #[cfg(feature = "network-controls")]
        apps.register(MedinoteRegistration::new(
            &crate::apps::network::descriptor::DESCRIPTOR,
            token(crate::apps::network::descriptor::PROVIDER_ID),
        ))
        .expect("network registers");

        let catalogue = build(home, &apps).expect("the product catalogue builds");
        assert_eq!(catalogue.len(), 1 + apps.len());
        assert!(apps.len() <= crate::apps::descriptor::MAX_APPS);
    }
}
