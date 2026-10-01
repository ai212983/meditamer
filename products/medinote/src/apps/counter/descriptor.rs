//! Counter's own [`AppDescriptor`] -- see `apps::hourglass::descriptor`'s
//! doc for why this lives beside the app it describes.

use shell::catalogue::{EntryId, GlyphRef};
use shell::types::{
    ProviderId, RefreshHint, SurfaceCapabilities, SurfaceId, SurfaceRole, SurfaceSpec,
};

use crate::apps::descriptor::{AppDescriptor, MedinoteApp};
use crate::catalogue::NAMESPACE;

pub const PROVIDER_ID: ProviderId = ProviderId(3);
pub const SURFACE_ID: SurfaceId = SurfaceId(6);
pub const ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 3);

const SURFACES: [SurfaceSpec; 1] = [SurfaceSpec::new(
    SURFACE_ID.0,
    SurfaceRole::AppRoot,
    SurfaceCapabilities::LAUNCHABLE,
    RefreshHint::Content,
)];

pub const DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MedinoteApp::Counter,
    entry_id: ENTRY_ID,
    provider: PROVIDER_ID,
    surfaces: &SURFACES,
    label: c"Counter",
    glyph: GlyphRef(3),
    capabilities: SurfaceCapabilities::LAUNCHABLE,
    default_rank: 1,
    launch_surface: SURFACE_ID,
};
