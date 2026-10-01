//! Hourglass's own [`AppDescriptor`] -- the provider/surface identity and
//! catalogue metadata `catalogue::build` and the target's registration code
//! need, in one place. Co-located with the app itself rather than in
//! `catalogue.rs`: adding a
//! second app should mean adding a descriptor beside *that* app, not
//! editing Medinote's one shared catalogue-construction file per app.

use shell::catalogue::{EntryId, GlyphRef};
use shell::types::{
    ProviderId, RefreshHint, SurfaceCapabilities, SurfaceId, SurfaceRole, SurfaceSpec,
};

use crate::apps::descriptor::{AppDescriptor, MedinoteApp};
use crate::catalogue::NAMESPACE;

pub const PROVIDER_ID: ProviderId = ProviderId(2);
pub const SURFACE_ID: SurfaceId = SurfaceId(3);
pub const ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 2);

const SURFACES: [SurfaceSpec; 1] = [SurfaceSpec::new(
    SURFACE_ID.0,
    SurfaceRole::AppRoot,
    SurfaceCapabilities::LAUNCHABLE,
    RefreshHint::Content,
)];

pub const DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MedinoteApp::Hourglass,
    entry_id: ENTRY_ID,
    provider: PROVIDER_ID,
    surfaces: &SURFACES,
    label: c"Hourglass",
    glyph: GlyphRef(2),
    capabilities: SurfaceCapabilities::LAUNCHABLE,
    default_rank: 0,
    launch_surface: SURFACE_ID,
};
