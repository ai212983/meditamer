use core::sync::atomic::{AtomicU32, Ordering};

use super::store::PersistedAppState;
use super::types::{DiagKind, DiagTargets, Phase, ServiceFlags};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct AppStateSnapshot {
    pub(crate) phase: Phase,
    pub(crate) services: ServiceFlags,
    pub(crate) diag_kind: DiagKind,
    pub(crate) diag_targets: DiagTargets,
}

impl Default for AppStateSnapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Initializing,
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
            services: ServiceFlags::normal(),
            diag_kind: DiagKind::None,
            diag_targets: DiagTargets::none(),
        }
    }

    const PHASE_SHIFT: u32 = 0;
    const SERVICES_SHIFT: u32 = 2;
    const DIAG_KIND_SHIFT: u32 = 3;
    const DIAG_TARGETS_SHIFT: u32 = 5;

    pub(crate) const fn packed(self) -> u32 {
        ((self.phase.as_u8() as u32) << Self::PHASE_SHIFT)
            | ((self.services.as_bits() as u32) << Self::SERVICES_SHIFT)
            | ((self.diag_kind.as_u8() as u32) << Self::DIAG_KIND_SHIFT)
            | ((self.diag_targets.as_persisted() as u32) << Self::DIAG_TARGETS_SHIFT)
    }

    pub(crate) fn from_packed(raw: u32) -> Self {
        let phase = Phase::from_u8(((raw >> Self::PHASE_SHIFT) & 0b11) as u8)
            .unwrap_or(Phase::Initializing);
        let services = ServiceFlags::from_bits(((raw >> Self::SERVICES_SHIFT) & 0b1) as u8);
        let diag_kind = DiagKind::from_u8(((raw >> Self::DIAG_KIND_SHIFT) & 0b11) as u8)
            .unwrap_or(DiagKind::None);
        let diag_targets =
            DiagTargets::from_persisted(((raw >> Self::DIAG_TARGETS_SHIFT) & 0b1_1111) as u8);

        Self {
            phase,
            services,
            diag_kind,
            diag_targets,
        }
    }

    pub(crate) fn from_persisted_sanitized(persisted: PersistedAppState) -> Self {
        Self {
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
    crate::firmware::runtime::scheduling::apply_snapshot(snapshot);
}

pub(crate) fn read_app_state_snapshot() -> AppStateSnapshot {
    AppStateSnapshot::from_packed(APP_STATE_SNAPSHOT.load(Ordering::Relaxed))
}

#[cfg(feature = "asset-upload-http")]
pub(crate) fn upload_enabled() -> bool {
    read_app_state_snapshot().services.upload_enabled
}
