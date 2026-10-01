use super::super::descriptor::{AppDescriptor, MedinoteApp};
use crate::catalogue::NAMESPACE;
use core::ffi::CStr;
use shell::catalogue::{EntryId, GlyphRef};
use shell::types::{
    ProviderId, RefreshHint, SurfaceCapabilities, SurfaceId, SurfaceRole, SurfaceSpec,
};

pub const ENTRY_ID: EntryId = EntryId::new(NAMESPACE, 4);
pub const PROVIDER_ID: ProviderId = ProviderId(4);
pub const SURFACE_ID: SurfaceId = SurfaceId(7);
pub const LABEL: &CStr = c"Wi-Fi";
pub const GLYPH: GlyphRef = GlyphRef(4);
pub const SURFACES: [SurfaceSpec; 1] = [SurfaceSpec::new(
    SURFACE_ID.0,
    SurfaceRole::AppRoot,
    SurfaceCapabilities::LAUNCHABLE,
    RefreshHint::Content,
)];
pub const DESCRIPTOR: AppDescriptor = AppDescriptor {
    kind: MedinoteApp::Network,
    entry_id: ENTRY_ID,
    provider: PROVIDER_ID,
    surfaces: &SURFACES,
    label: LABEL,
    glyph: GLYPH,
    capabilities: SurfaceCapabilities::LAUNCHABLE,
    default_rank: 3,
    launch_surface: SURFACE_ID,
};

pub const APP: AppDescriptor = DESCRIPTOR;
