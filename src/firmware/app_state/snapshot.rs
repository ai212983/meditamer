use core::sync::atomic::{AtomicU32, Ordering};

use super::store::PersistedAppState;
use super::types::{
    BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode, Phase, ServiceFlags,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct AppStateSnapshot {
    pub(crate) phase: Phase,
    pub(crate) base: BaseMode,
    pub(crate) day_background: DayBackground,
    pub(crate) overlay: OverlayMode,
    pub(crate) services: ServiceFlags,
    pub(crate) diag_kind: DiagKind,
    pub(crate) diag_targets: DiagTargets,
}

impl Default for AppStateSnapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Initializing,
            base: BaseMode::Day,
            day_background: DayBackground::Shanshui,
            overlay: OverlayMode::None,
            services: ServiceFlags::normal(),
            diag_kind: DiagKind::None,
            diag_targets: DiagTargets::none(),
        }
    }
}

impl AppStateSnapshot {
    pub(crate) const fn default_const() -> Self {
        Self {
            phase: Phase::Initializing,
            base: BaseMode::Day,
            day_background: DayBackground::Shanshui,
            overlay: OverlayMode::None,
            services: ServiceFlags::normal(),
            diag_kind: DiagKind::None,
            diag_targets: DiagTargets::none(),
        }
    }

    const PHASE_SHIFT: u32 = 0;
    const BASE_SHIFT: u32 = 2;
    const DAY_BG_SHIFT: u32 = 4;
    const OVERLAY_SHIFT: u32 = 5;
    const SERVICES_SHIFT: u32 = 6;
    const DIAG_KIND_SHIFT: u32 = 8;
    const DIAG_TARGETS_SHIFT: u32 = 10;

    pub(crate) const fn packed(self) -> u32 {
        ((self.phase.as_u8() as u32) << Self::PHASE_SHIFT)
            | ((self.base.as_u8() as u32) << Self::BASE_SHIFT)
            | ((self.day_background.as_u8() as u32) << Self::DAY_BG_SHIFT)
            | ((self.overlay.as_u8() as u32) << Self::OVERLAY_SHIFT)
            | ((self.services.as_bits() as u32) << Self::SERVICES_SHIFT)
            | ((self.diag_kind.as_u8() as u32) << Self::DIAG_KIND_SHIFT)
            | ((self.diag_targets.as_persisted() as u32) << Self::DIAG_TARGETS_SHIFT)
    }

    pub(crate) fn from_packed(raw: u32) -> Self {
        let phase = Phase::from_u8(((raw >> Self::PHASE_SHIFT) & 0b11) as u8)
            .unwrap_or(Phase::Initializing);
        let base =
            BaseMode::from_u8(((raw >> Self::BASE_SHIFT) & 0b11) as u8).unwrap_or(BaseMode::Day);
        let day_background = DayBackground::from_u8(((raw >> Self::DAY_BG_SHIFT) & 0b1) as u8)
            .unwrap_or(DayBackground::Shanshui);
        let overlay = OverlayMode::from_u8(((raw >> Self::OVERLAY_SHIFT) & 0b1) as u8)
            .unwrap_or(OverlayMode::None);
        let services = ServiceFlags::from_bits(((raw >> Self::SERVICES_SHIFT) & 0b11) as u8);
        let diag_kind = DiagKind::from_u8(((raw >> Self::DIAG_KIND_SHIFT) & 0b11) as u8)
            .unwrap_or(DiagKind::None);
        let diag_targets =
            DiagTargets::from_persisted(((raw >> Self::DIAG_TARGETS_SHIFT) & 0b1_1111) as u8);

        Self {
            phase,
            base,
            day_background,
            overlay,
            services,
            diag_kind,
            diag_targets,
        }
    }

    pub(crate) fn from_persisted_sanitized(persisted: PersistedAppState) -> Self {
        let overlay = if matches!(persisted.base, BaseMode::TouchWizard) {
            OverlayMode::None
        } else {
            persisted.overlay
        };
        Self {
            base: persisted.base,
            day_background: persisted.day_background,
            overlay,
            services: persisted.services,
            diag_kind: persisted.diag_kind,
            diag_targets: persisted.diag_targets,
            ..Self::default()
        }
    }
}

static APP_STATE_SNAPSHOT: AtomicU32 = AtomicU32::new(AppStateSnapshot::default_const().packed());

pub(crate) fn publish_app_state_snapshot(snapshot: AppStateSnapshot) {
    APP_STATE_SNAPSHOT.store(snapshot.packed(), Ordering::Relaxed);
}

pub(crate) fn read_app_state_snapshot() -> AppStateSnapshot {
    AppStateSnapshot::from_packed(APP_STATE_SNAPSHOT.load(Ordering::Relaxed))
}

pub(crate) fn upload_enabled() -> bool {
    read_app_state_snapshot().services.upload_enabled
}
