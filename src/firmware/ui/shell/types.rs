#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProviderId(pub(crate) u16);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProviderGeneration(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProviderToken {
    pub(crate) id: ProviderId,
    pub(crate) generation: ProviderGeneration,
}

impl ProviderToken {
    pub(super) const fn issued(id: ProviderId, generation: ProviderGeneration) -> Self {
        Self { id, generation }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SurfaceId(pub(crate) u16);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SurfaceRef {
    pub(crate) owner: ProviderToken,
    pub(crate) id: SurfaceId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct InstanceGeneration(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct SurfaceInstanceToken {
    pub(crate) surface: SurfaceRef,
    pub(crate) generation: InstanceGeneration,
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
    pub(crate) const fn new(owner: ProviderToken, id: u16) -> Self {
        Self {
            owner,
            id: SurfaceId(id),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceSpec {
    pub(crate) id: SurfaceId,
    pub(crate) role: SurfaceRole,
    pub(crate) capabilities: SurfaceCapabilities,
    pub(crate) refresh_hint: RefreshHint,
}

impl SurfaceSpec {
    pub(crate) const fn new(
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
pub(crate) enum SurfaceRole {
    Ambient,
    Launcher,
    AppRoot,
    AppChild,
    SystemRoot,
    Overlay,
}

impl SurfaceRole {
    pub(crate) const fn is_launch_root(self) -> bool {
        matches!(self, Self::AppRoot | Self::SystemRoot)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceCapabilities(u8);

impl SurfaceCapabilities {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const LAUNCHABLE: Self = Self(1 << 0);
    pub(crate) const AMBIENT: Self = Self(1 << 1);
    pub(crate) const OVERLAY: Self = Self(1 << 2);

    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RefreshHint {
    Micro,
    Content,
    Boundary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceDefinition {
    pub(crate) surface: SurfaceRef,
    pub(crate) role: SurfaceRole,
    pub(crate) capabilities: SurfaceCapabilities,
    pub(crate) refresh_hint: RefreshHint,
}

impl SurfaceDefinition {
    pub(crate) const fn new(
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
pub(crate) enum OverlayInput {
    Passive,
    Interactive,
    Modal,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayLifetime {
    Transient,
    Sticky,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum OverlayBand {
    Provider,
    BaseSystem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlayInstance {
    pub(crate) token: SurfaceInstanceToken,
    pub(crate) request_owner: ProviderToken,
    pub(crate) band: OverlayBand,
    pub(crate) input: OverlayInput,
    pub(crate) lifetime: OverlayLifetime,
    pub(crate) rank: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverlayAdmission {
    Active(OverlayInstance),
    Queued(OverlayInstance),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverlayDismissal {
    pub(crate) removed: OverlayInstance,
    pub(crate) removed_was_live: bool,
    pub(crate) promoted: Option<OverlayInstance>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NavIntent {
    OpenLauncher(SurfaceRef),
    Launch(SurfaceRef),
    Push(SurfaceRef),
    Back,
    Home,
}

impl NavIntent {
    pub(crate) const fn references_provider(self, owner: ProviderToken) -> bool {
        match self {
            Self::OpenLauncher(surface) | Self::Launch(surface) | Self::Push(surface) => {
                surface.owner.id.0 == owner.id.0 && surface.owner.generation.0 == owner.generation.0
            }
            Self::Back | Self::Home => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OwnedNavIntent {
    pub(crate) source: SurfaceInstanceToken,
    pub(crate) intent: NavIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompositionIntent {
    Request {
        surface: SurfaceRef,
        input: OverlayInput,
        lifetime: OverlayLifetime,
        rank: u8,
    },
    DismissActiveModal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OwnedCompositionIntent {
    pub(crate) source: SurfaceInstanceToken,
    pub(crate) intent: CompositionIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RefreshIntent {
    FullRepaint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OwnedRefreshIntent {
    pub(crate) source: SurfaceInstanceToken,
    pub(crate) intent: RefreshIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OwnedUiSettingsIntent {
    pub(crate) source: SurfaceInstanceToken,
    pub(crate) intent: super::settings::UiSettingsIntent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnedShellIntent {
    Navigate(OwnedNavIntent),
    Compose(OwnedCompositionIntent),
    Refresh(OwnedRefreshIntent),
    Configure(OwnedUiSettingsIntent),
}

impl OwnedShellIntent {
    pub(crate) const fn source(self) -> SurfaceInstanceToken {
        match self {
            Self::Navigate(intent) => intent.source,
            Self::Compose(intent) => intent.source,
            Self::Refresh(intent) => intent.source,
            Self::Configure(intent) => intent.source,
        }
    }

    pub(crate) const fn references_provider(self, owner: ProviderToken) -> bool {
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
            Self::Refresh(intent) => {
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
