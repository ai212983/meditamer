use heapless::Vec;

use super::catalogue::{CatalogueViewKind, CompiledCatalogue, EntryId, CATALOGUE_CAPACITY};

pub const UI_SETTINGS_CAPACITY: usize = CATALOGUE_CAPACITY;
pub const UI_SETTINGS_WRITE_DEBOUNCE_MS: u64 = 1_500;
pub const UI_SETTINGS_MIN_WRITE_INTERVAL_MS: u64 = 5_000;
pub const UI_SETTINGS_RETRY_BACKOFF_MS: u64 = 30_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedUiSettings {
    pub ambient_binding: Option<EntryId>,
    pub pins: Vec<EntryId, UI_SETTINGS_CAPACITY>,
    pub enabled_overlays: Vec<EntryId, UI_SETTINGS_CAPACITY>,
    pub startup_entry: Option<EntryId>,
    pub startup_overlays: Vec<EntryId, UI_SETTINGS_CAPACITY>,
    pub enablement_configured: bool,
    pub startup_overlays_configured: bool,
}

impl Default for PersistedUiSettings {
    fn default() -> Self {
        Self {
            ambient_binding: None,
            pins: Vec::new(),
            enabled_overlays: Vec::new(),
            startup_entry: None,
            startup_overlays: Vec::new(),
            enablement_configured: false,
            startup_overlays_configured: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiSettings {
    persisted: PersistedUiSettings,
}

impl UiSettings {
    pub fn resolve<const CAPACITY: usize>(
        catalogue: &CompiledCatalogue<CAPACITY>,
        persisted: PersistedUiSettings,
        default_startup_overlays: &[EntryId],
    ) -> Self {
        let ambient_binding = persisted
            .ambient_binding
            .filter(|id| catalogue.entry_is_ready_for(*id, CatalogueViewKind::AmbientPicker))
            .or_else(|| Some(catalogue.ambient_fallback().id));

        let pins = sanitized_ids(catalogue, &persisted.pins, CatalogueViewKind::Launcher);
        let enabled_source = if persisted.enablement_configured {
            persisted.enabled_overlays.as_slice()
        } else {
            default_startup_overlays
        };
        let enabled_overlays = sanitized_ids(
            catalogue,
            enabled_source,
            CatalogueViewKind::OverlaySettings,
        );
        let startup_entry = persisted
            .startup_entry
            .filter(|id| catalogue.entry_is_ready_for(*id, CatalogueViewKind::Launcher));
        let startup_source = if persisted.startup_overlays_configured {
            persisted.startup_overlays.as_slice()
        } else {
            default_startup_overlays
        };
        let mut startup_overlays = sanitized_ids(
            catalogue,
            startup_source,
            CatalogueViewKind::OverlaySettings,
        );
        startup_overlays.retain(|id| enabled_overlays.contains(id));

        Self {
            persisted: PersistedUiSettings {
                ambient_binding,
                pins,
                enabled_overlays,
                startup_entry,
                startup_overlays,
                enablement_configured: persisted.enablement_configured,
                startup_overlays_configured: persisted.startup_overlays_configured,
            },
        }
    }

    pub fn persisted(&self) -> PersistedUiSettings {
        self.persisted.clone()
    }

    pub fn ambient_binding(&self) -> EntryId {
        self.persisted
            .ambient_binding
            .expect("settings resolution always supplies the base ambient fallback")
    }

    pub fn pins(&self) -> &[EntryId] {
        self.persisted.pins.as_slice()
    }

    pub fn overlay_enabled(&self, id: EntryId) -> bool {
        self.persisted.enabled_overlays.contains(&id)
    }

    pub fn startup_entry(&self) -> Option<EntryId> {
        self.persisted.startup_entry
    }

    pub fn startup_overlay_enabled(&self, id: EntryId) -> bool {
        self.persisted.startup_overlays.contains(&id)
    }

    pub fn select_ambient(&mut self, id: EntryId) -> bool {
        if self.persisted.ambient_binding == Some(id) {
            return false;
        }
        self.persisted.ambient_binding = Some(id);
        true
    }

    pub fn toggle_overlay(&mut self, id: EntryId) -> bool {
        self.persisted.enablement_configured = true;
        self.persisted.startup_overlays_configured = true;
        if let Some(index) = self
            .persisted
            .enabled_overlays
            .iter()
            .position(|candidate| *candidate == id)
        {
            self.persisted.enabled_overlays.remove(index);
            self.persisted
                .startup_overlays
                .retain(|candidate| *candidate != id);
            false
        } else {
            let _ = self.persisted.enabled_overlays.push(id);
            if !self.persisted.startup_overlays.contains(&id) {
                let _ = self.persisted.startup_overlays.push(id);
            }
            true
        }
    }
}

fn sanitized_ids<const CAPACITY: usize>(
    catalogue: &CompiledCatalogue<CAPACITY>,
    source: &[EntryId],
    view: CatalogueViewKind,
) -> Vec<EntryId, UI_SETTINGS_CAPACITY> {
    let mut sanitized = Vec::new();
    for id in source.iter().copied() {
        if sanitized.contains(&id)
            || !catalogue.entry_is_ready_for(id, view)
            || sanitized.push(id).is_err()
        {
            continue;
        }
    }
    sanitized
}

pub struct UiSettingsPersistence {
    current: UiSettings,
    committed: PersistedUiSettings,
    in_flight: Option<PersistedUiSettings>,
    dirty: bool,
    write_not_before_ms: u64,
}

impl UiSettingsPersistence {
    pub fn new(current: UiSettings) -> Self {
        let committed = current.persisted();
        Self {
            current,
            committed,
            in_flight: None,
            dirty: false,
            write_not_before_ms: 0,
        }
    }

    pub fn current(&self) -> &UiSettings {
        &self.current
    }

    pub fn select_ambient(&mut self, id: EntryId, now_ms: u64) -> bool {
        let changed = self.current.select_ambient(id);
        if changed {
            self.mark_dirty(now_ms);
        }
        changed
    }

    pub fn toggle_overlay(&mut self, id: EntryId, now_ms: u64) -> Option<bool> {
        let before = self.current.persisted();
        let enabled = self.current.toggle_overlay(id);
        if self.current.persisted() == before {
            return None;
        }
        self.mark_dirty(now_ms);
        Some(enabled)
    }

    pub fn take_due(&mut self, now_ms: u64) -> Option<PersistedUiSettings> {
        if !self.dirty || self.in_flight.is_some() || now_ms < self.write_not_before_ms {
            return None;
        }
        let candidate = self.current.persisted();
        self.in_flight = Some(candidate.clone());
        Some(candidate)
    }

    pub fn complete(&mut self, success: bool, now_ms: u64) {
        let Some(attempted) = self.in_flight.take() else {
            return;
        };
        if success {
            self.committed = attempted;
            self.dirty = self.current.persisted() != self.committed;
            self.write_not_before_ms = now_ms.saturating_add(UI_SETTINGS_MIN_WRITE_INTERVAL_MS);
        } else {
            self.dirty = true;
            self.write_not_before_ms = now_ms.saturating_add(UI_SETTINGS_RETRY_BACKOFF_MS);
        }
    }

    fn mark_dirty(&mut self, now_ms: u64) {
        self.dirty = self.current.persisted() != self.committed;
        if self.dirty && self.in_flight.is_none() {
            self.write_not_before_ms = self
                .write_not_before_ms
                .max(now_ms.saturating_add(UI_SETTINGS_WRITE_DEBOUNCE_MS));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiSettingsIntent {
    SelectAmbient(EntryId),
    ToggleOverlay(EntryId),
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;
    use core::ffi::CStr;

    use crate::catalogue::CatalogueAvailability;
    use crate::{
        catalogue::{CatalogueEntry, GlyphRef},
        types::{ProviderGeneration, ProviderId, ProviderToken, SurfaceCapabilities, SurfaceRef},
    };

    const HOME_ID: EntryId = EntryId::new(1, 1);
    const APP_ID: EntryId = EntryId::new(1, 2);
    const OVERLAY_ID: EntryId = EntryId::new(1, 3);

    fn entry(
        id: EntryId,
        label: &'static CStr,
        capabilities: SurfaceCapabilities,
    ) -> CatalogueEntry {
        CatalogueEntry {
            id,
            label,
            glyph: GlyphRef(id.local),
            surface: SurfaceRef::new(
                ProviderToken {
                    id: ProviderId(1),
                    generation: ProviderGeneration(1),
                },
                id.local,
            ),
            capabilities,
            default_rank: id.local as u8,
            pin: None,
            availability: CatalogueAvailability::Ready,
        }
    }

    fn catalogue() -> CompiledCatalogue<4> {
        CompiledCatalogue::new(
            &[
                entry(HOME_ID, c"Home", SurfaceCapabilities::AMBIENT),
                entry(APP_ID, c"App", SurfaceCapabilities::LAUNCHABLE),
                entry(OVERLAY_ID, c"Overlay", SurfaceCapabilities::OVERLAY),
            ],
            HOME_ID,
        )
        .unwrap()
    }

    #[test]
    fn resolution_drops_unknown_unavailable_and_duplicate_ids() {
        let mut persisted = PersistedUiSettings {
            ambient_binding: Some(EntryId::new(9, 9)),
            enablement_configured: true,
            startup_overlays_configured: true,
            ..PersistedUiSettings::default()
        };
        persisted
            .pins
            .extend_from_slice(&[APP_ID, APP_ID, OVERLAY_ID])
            .unwrap();
        persisted
            .enabled_overlays
            .extend_from_slice(&[EntryId::new(9, 9), OVERLAY_ID, OVERLAY_ID])
            .unwrap();
        persisted
            .startup_overlays
            .extend_from_slice(&[OVERLAY_ID, EntryId::new(9, 9)])
            .unwrap();

        let resolved = UiSettings::resolve(&catalogue(), persisted, &[OVERLAY_ID]);
        assert_eq!(resolved.ambient_binding(), HOME_ID);
        assert_eq!(resolved.pins(), &[APP_ID]);
        assert!(resolved.overlay_enabled(OVERLAY_ID));
        assert!(resolved.startup_overlay_enabled(OVERLAY_ID));
    }

    #[test]
    fn write_rate_coalesces_and_backs_off_after_failure() {
        let settings =
            UiSettings::resolve(&catalogue(), PersistedUiSettings::default(), &[OVERLAY_ID]);
        let mut persistence = UiSettingsPersistence::new(settings);
        assert_eq!(persistence.toggle_overlay(OVERLAY_ID, 100), Some(false));
        assert!(persistence.take_due(1_599).is_none());
        assert!(persistence.take_due(1_600).is_some());
        persistence.complete(false, 1_600);
        assert!(persistence.take_due(31_599).is_none());
        assert!(persistence.take_due(31_600).is_some());
        persistence.complete(true, 31_600);
        assert!(persistence.take_due(u64::MAX).is_none());
    }

    #[test]
    fn first_overlay_edit_materializes_compiled_defaults() {
        let settings =
            UiSettings::resolve(&catalogue(), PersistedUiSettings::default(), &[OVERLAY_ID]);
        let mut persistence = UiSettingsPersistence::new(settings);
        assert!(persistence.current().overlay_enabled(OVERLAY_ID));
        assert_eq!(persistence.toggle_overlay(OVERLAY_ID, 0), Some(false));
        let stored = persistence.take_due(UI_SETTINGS_WRITE_DEBOUNCE_MS).unwrap();
        assert!(stored.enablement_configured);
        assert!(stored.startup_overlays_configured);
        assert!(stored.enabled_overlays.is_empty());
        assert!(stored.startup_overlays.is_empty());
    }

    #[test]
    fn unavailable_startup_entry_falls_back_to_ambient() {
        let persisted = PersistedUiSettings {
            startup_entry: Some(EntryId::new(9, 9)),
            ..PersistedUiSettings::default()
        };
        let resolved = UiSettings::resolve(&catalogue(), persisted, &[OVERLAY_ID]);
        assert_eq!(resolved.startup_entry(), None);
        assert_eq!(resolved.ambient_binding(), HOME_ID);
    }

    #[test]
    fn ready_filter_requires_the_requested_capability() {
        let catalogue = catalogue();
        assert!(catalogue.entry_is_ready_for(APP_ID, CatalogueViewKind::Launcher));
        assert!(!catalogue.entry_is_ready_for(APP_ID, CatalogueViewKind::AmbientPicker));
        assert_eq!(
            catalogue.entry(APP_ID).unwrap().availability,
            CatalogueAvailability::Ready
        );
    }
}
