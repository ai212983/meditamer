//! Meditamer's product apps and their shell-facing descriptions.
//!
//! The current launchable tools share the base UI provider. That provider
//! topology is independent of their product identity: each tool is still a
//! distinct app in [`MeditamerApps`], and a future Gallery may use either the
//! base provider or its own provider without changing app lookup.

use shell::apps::{AppRegistration, ProductApps, ProductAppsError};
use shell::catalogue::{EntryId, GlyphRef};
use shell::types::{
    ProviderId, ProviderToken, RefreshHint, SurfaceCapabilities, SurfaceId, SurfaceRole,
    SurfaceSpec,
};

/// Analog-clock catalogue identity, single-sourced from the screen model so
/// the entry/surface IDs cannot drift from the tested cadence contract.
use super::screen::analog_clock::{
    ENTRY_LOCAL as CLOCK_ENTRY_LOCAL, ENTRY_NAMESPACE as CLOCK_NAMESPACE,
    SURFACE_ID as CLOCK_SURFACE_ID,
};

pub(crate) const NAMESPACE: u16 = 1;
pub(crate) const BASE_PROVIDER_ID: ProviderId = ProviderId(1);

const _: () = assert!(NAMESPACE == CLOCK_NAMESPACE);

pub(crate) const DIAGNOSTICS_SURFACE_ID: SurfaceId = SurfaceId(3);
pub(crate) const AMBIENT_VIEW_SURFACE_ID: SurfaceId = SurfaceId(7);
pub(crate) const OVERLAY_TOGGLES_SURFACE_ID: SurfaceId = SurfaceId(8);
pub(crate) const ANALOG_CLOCK_SURFACE_ID: SurfaceId = SurfaceId(CLOCK_SURFACE_ID);

pub(crate) const DIAGNOSTICS_SURFACE: SurfaceSpec = SurfaceSpec::new(
    DIAGNOSTICS_SURFACE_ID.0,
    SurfaceRole::SystemRoot,
    SurfaceCapabilities::LAUNCHABLE,
    RefreshHint::Boundary,
);
pub(crate) const AMBIENT_VIEW_SURFACE: SurfaceSpec = SurfaceSpec::new(
    AMBIENT_VIEW_SURFACE_ID.0,
    SurfaceRole::SystemRoot,
    SurfaceCapabilities::AMBIENT.union(SurfaceCapabilities::LAUNCHABLE),
    RefreshHint::Boundary,
);
pub(crate) const ANALOG_CLOCK_SURFACE: SurfaceSpec = SurfaceSpec::new(
    ANALOG_CLOCK_SURFACE_ID.0,
    SurfaceRole::SystemRoot,
    SurfaceCapabilities::AMBIENT.union(SurfaceCapabilities::LAUNCHABLE),
    RefreshHint::Boundary,
);
pub(crate) const OVERLAY_TOGGLES_SURFACE: SurfaceSpec = SurfaceSpec::new(
    OVERLAY_TOGGLES_SURFACE_ID.0,
    SurfaceRole::SystemRoot,
    SurfaceCapabilities::LAUNCHABLE,
    RefreshHint::Boundary,
);

const DIAGNOSTICS_SURFACES: [SurfaceSpec; 1] = [DIAGNOSTICS_SURFACE];
const AMBIENT_VIEW_SURFACES: [SurfaceSpec; 1] = [AMBIENT_VIEW_SURFACE];
const OVERLAY_TOGGLES_SURFACES: [SurfaceSpec; 1] = [OVERLAY_TOGGLES_SURFACE];
const ANALOG_CLOCK_SURFACES: [SurfaceSpec; 1] = [ANALOG_CLOCK_SURFACE];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MeditamerApp {
    GestureDiagnostics,
    AmbientView,
    OverlayToggles,
    AnalogClock,
}

pub(crate) type AppDescriptor = shell::apps::AppDescriptor<MeditamerApp>;
pub(crate) type MeditamerApps = ProductApps<MeditamerApp, 4>;

pub(crate) const DIAGNOSTICS_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 2);
pub(crate) const AMBIENT_VIEW_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 3);
pub(crate) const OVERLAY_TOGGLES_ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 4);
pub(crate) const ANALOG_CLOCK_ENTRY_ID: EntryId = EntryId::new(CLOCK_NAMESPACE, CLOCK_ENTRY_LOCAL);

pub(crate) const DIAGNOSTICS_DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MeditamerApp::GestureDiagnostics,
    entry_id: DIAGNOSTICS_ENTRY_ID,
    provider: BASE_PROVIDER_ID,
    surfaces: &DIAGNOSTICS_SURFACES,
    label: c"Gesture diagnostics",
    glyph: GlyphRef(2),
    capabilities: SurfaceCapabilities::LAUNCHABLE,
    default_rank: 0,
    launch_surface: DIAGNOSTICS_SURFACE_ID,
};

pub(crate) const AMBIENT_VIEW_DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MeditamerApp::AmbientView,
    entry_id: AMBIENT_VIEW_ENTRY_ID,
    provider: BASE_PROVIDER_ID,
    surfaces: &AMBIENT_VIEW_SURFACES,
    label: c"Ambient view",
    glyph: GlyphRef(3),
    capabilities: SurfaceCapabilities::AMBIENT.union(SurfaceCapabilities::LAUNCHABLE),
    default_rank: 1,
    launch_surface: AMBIENT_VIEW_SURFACE_ID,
};

pub(crate) const ANALOG_CLOCK_DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MeditamerApp::AnalogClock,
    entry_id: ANALOG_CLOCK_ENTRY_ID,
    provider: BASE_PROVIDER_ID,
    surfaces: &ANALOG_CLOCK_SURFACES,
    label: c"Analog clock",
    glyph: GlyphRef(6),
    capabilities: SurfaceCapabilities::AMBIENT.union(SurfaceCapabilities::LAUNCHABLE),
    default_rank: 3,
    launch_surface: ANALOG_CLOCK_SURFACE_ID,
};

pub(crate) const OVERLAY_TOGGLES_DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MeditamerApp::OverlayToggles,
    entry_id: OVERLAY_TOGGLES_ENTRY_ID,
    provider: BASE_PROVIDER_ID,
    surfaces: &OVERLAY_TOGGLES_SURFACES,
    label: c"Overlay toggles",
    glyph: GlyphRef(4),
    capabilities: SurfaceCapabilities::LAUNCHABLE,
    default_rank: 2,
    launch_surface: OVERLAY_TOGGLES_SURFACE_ID,
};

const DESCRIPTORS: [&AppDescriptor; 4] = [
    &DIAGNOSTICS_DESCRIPTOR,
    &AMBIENT_VIEW_DESCRIPTOR,
    &OVERLAY_TOGGLES_DESCRIPTOR,
    &ANALOG_CLOCK_DESCRIPTOR,
];

pub(crate) fn register_apps(owner: ProviderToken) -> Result<MeditamerApps, ProductAppsError> {
    let mut apps = MeditamerApps::new();
    for descriptor in DESCRIPTORS {
        apps.register(AppRegistration::new(descriptor, owner))?;
    }
    Ok(apps)
}
