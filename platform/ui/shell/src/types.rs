#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderId(pub u16);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderGeneration(pub u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProviderToken {
    pub id: ProviderId,
    pub generation: ProviderGeneration,
}

impl ProviderToken {
    pub(super) const fn issued(id: ProviderId, generation: ProviderGeneration) -> Self {
        Self { id, generation }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SurfaceId(pub u16);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SurfaceRef {
    pub owner: ProviderToken,
    pub id: SurfaceId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InstanceGeneration(pub u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SurfaceInstanceToken {
    pub surface: SurfaceRef,
    pub generation: InstanceGeneration,
}

impl SurfaceInstanceToken {
    pub(super) const fn issued(surface: SurfaceRef, generation: InstanceGeneration) -> Self {
        Self {
            surface,
            generation,
        }
    }
}

impl SurfaceRef {
    pub const fn new(owner: ProviderToken, id: u16) -> Self {
        Self {
            owner,
            id: SurfaceId(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceSpec {
    pub id: SurfaceId,
    pub role: SurfaceRole,
    pub capabilities: SurfaceCapabilities,
    pub refresh_hint: RefreshHint,
}

impl SurfaceSpec {
    pub const fn new(
        id: u16,
        role: SurfaceRole,
        capabilities: SurfaceCapabilities,
        refresh_hint: RefreshHint,
    ) -> Self {
        Self {
            id: SurfaceId(id),
            role,
            capabilities,
            refresh_hint,
        }
    }

    pub(super) const fn with_owner(self, owner: ProviderToken) -> SurfaceDefinition {
        SurfaceDefinition::new(
            SurfaceRef { owner, id: self.id },
            self.role,
            self.capabilities,
            self.refresh_hint,
        )
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceRole {
    Ambient,
    Launcher,
    AppRoot,
    AppChild,
    SystemRoot,
    Overlay,
}

impl SurfaceRole {
    pub const fn is_launch_root(self) -> bool {
        matches!(self, Self::AppRoot | Self::SystemRoot)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceCapabilities(u8);

impl SurfaceCapabilities {
    pub const NONE: Self = Self(0);
    pub const LAUNCHABLE: Self = Self(1 << 0);
    pub const AMBIENT: Self = Self(1 << 1);
    pub const OVERLAY: Self = Self(1 << 2);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RefreshHint {
    Micro,
    Content,
    Boundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SurfaceDefinition {
    pub surface: SurfaceRef,
    pub role: SurfaceRole,
    pub capabilities: SurfaceCapabilities,
    pub refresh_hint: RefreshHint,
}

impl SurfaceDefinition {
    pub const fn new(
        surface: SurfaceRef,
        role: SurfaceRole,
        capabilities: SurfaceCapabilities,
        refresh_hint: RefreshHint,
    ) -> Self {
        Self {
            surface,
            role,
            capabilities,
            refresh_hint,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayInput {
    Passive,
    Interactive,
    Modal,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayLifetime {
    Transient,
    Sticky,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum OverlayBand {
    Provider,
    BaseSystem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayInstance {
    pub token: SurfaceInstanceToken,
    pub request_owner: ProviderToken,
    pub band: OverlayBand,
    pub input: OverlayInput,
    pub lifetime: OverlayLifetime,
    pub rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayAdmission {
    Active(OverlayInstance),
    Queued(OverlayInstance),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverlayDismissal {
    pub removed: OverlayInstance,
    pub removed_was_live: bool,
    pub promoted: Option<OverlayInstance>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavIntent {
    OpenLauncher(SurfaceRef),
    Launch(SurfaceRef),
    Push(SurfaceRef),
    Back,
    Home,
}

impl NavIntent {
    pub const fn references_provider(self, owner: ProviderToken) -> bool {
        match self {
            Self::OpenLauncher(surface) | Self::Launch(surface) | Self::Push(surface) => {
                surface.owner.id.0 == owner.id.0 && surface.owner.generation.0 == owner.generation.0
            }
            Self::Back | Self::Home => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedNavIntent {
    pub source: SurfaceInstanceToken,
    pub intent: NavIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionIntent {
    Request {
        surface: SurfaceRef,
        input: OverlayInput,
        lifetime: OverlayLifetime,
        rank: u8,
    },
    DismissActiveModal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedCompositionIntent {
    pub source: SurfaceInstanceToken,
    pub intent: CompositionIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshIntent {
    FullRepaint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedRefreshIntent {
    pub source: SurfaceInstanceToken,
    pub intent: RefreshIntent,
}

/// Presentation policy selected by UI work, independently of a renderer or
/// panel waveform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenUpdateIntent {
    /// Request the quickest safe, flash-free presentation.
    Fast,
    /// Request a clean reconstruction, accepting a slower visible update.
    Clean,
}

impl ScreenUpdateIntent {
    pub const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Clean, _) | (_, Self::Clean) => Self::Clean,
            (Self::Fast, Self::Fast) => Self::Fast,
        }
    }
}

/// A renderer-independent request to present the current screen state.
///
/// Renderer damage deliberately does not live here: it is geometry produced
/// by a renderer, while `shell` remains independent of renderer and product
/// types. The boundary that owns both may pair this semantic request with
/// optional damage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenUpdateRequest {
    pub intent: ScreenUpdateIntent,
}

impl ScreenUpdateRequest {
    /// Pair semantic presentation policy with damage produced by a renderer.
    /// The generic keeps `shell` independent of any renderer geometry type.
    pub const fn with_damage<Damage>(self, damage: Option<Damage>) -> ScreenUpdate<Damage> {
        ScreenUpdate {
            intent: self.intent,
            damage,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenUpdate<Damage> {
    pub intent: ScreenUpdateIntent,
    pub damage: Option<Damage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedScreenUpdateRequest {
    pub source: SurfaceInstanceToken,
    pub request: ScreenUpdateRequest,
}

impl From<OwnedRefreshIntent> for OwnedScreenUpdateRequest {
    fn from(refresh: OwnedRefreshIntent) -> Self {
        let intent = match refresh.intent {
            RefreshIntent::FullRepaint => ScreenUpdateIntent::Clean,
        };
        Self {
            source: refresh.source,
            request: ScreenUpdateRequest { intent },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnedUiSettingsIntent {
    pub source: SurfaceInstanceToken,
    pub intent: super::settings::UiSettingsIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedShellIntent {
    Navigate(OwnedNavIntent),
    Compose(OwnedCompositionIntent),
    ScreenUpdate(OwnedScreenUpdateRequest),
    Configure(OwnedUiSettingsIntent),
}

impl OwnedShellIntent {
    pub const fn source(self) -> SurfaceInstanceToken {
        match self {
            Self::Navigate(intent) => intent.source,
            Self::Compose(intent) => intent.source,
            Self::ScreenUpdate(intent) => intent.source,
            Self::Configure(intent) => intent.source,
        }
    }

    pub const fn references_provider(self, owner: ProviderToken) -> bool {
        match self {
            Self::Navigate(intent) => {
                intent.source.surface.owner.id.0 == owner.id.0
                    && intent.source.surface.owner.generation.0 == owner.generation.0
                    || intent.intent.references_provider(owner)
            }
            Self::Compose(intent) => {
                if intent.source.surface.owner.id.0 == owner.id.0
                    && intent.source.surface.owner.generation.0 == owner.generation.0
                {
                    return true;
                }
                match intent.intent {
                    CompositionIntent::Request { surface, .. } => {
                        surface.owner.id.0 == owner.id.0
                            && surface.owner.generation.0 == owner.generation.0
                    }
                    CompositionIntent::DismissActiveModal => false,
                }
            }
            Self::ScreenUpdate(intent) => {
                intent.source.surface.owner.id.0 == owner.id.0
                    && intent.source.surface.owner.generation.0 == owner.generation.0
            }
            Self::Configure(intent) => {
                intent.source.surface.owner.id.0 == owner.id.0
                    && intent.source.surface.owner.generation.0 == owner.generation.0
            }
        }
    }
}
