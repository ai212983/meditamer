use statig::blocking::IntoStateMachineExt as _;

use super::actions::{AppStateApplyStatus, AppStateDiagControl};
use super::events::AppStateCommand;
use super::machine::{AppStateMachine, DispatchContext};
use super::snapshot::AppStateSnapshot;
use super::store::PersistedAppState;
use super::types::{DiagKind, Phase};

#[derive(Clone, Copy, Debug)]
pub(crate) struct AppStateApplyResult {
    pub(crate) before: AppStateSnapshot,
    pub(crate) after: AppStateSnapshot,
    pub(crate) status: AppStateApplyStatus,
}

impl AppStateApplyResult {
    pub(crate) fn changed(self) -> bool {
        matches!(self.status, AppStateApplyStatus::Applied)
    }

    pub(crate) fn persist_required(self) -> bool {
        if !self.changed() {
            return false;
        }
        PersistedAppState::from_snapshot(self.before)
            != PersistedAppState::from_snapshot(self.after)
    }

    pub(crate) fn services_changed(self) -> bool {
        self.before.services != self.after.services
    }

    pub(crate) fn diag_control(self) -> Option<AppStateDiagControl> {
        let before_kind = self.before.diag_kind;
        let after_kind = self.after.diag_kind;

        if !matches!(before_kind, DiagKind::None) && matches!(after_kind, DiagKind::None) {
            return Some(AppStateDiagControl::Stop);
        }

        if matches!(after_kind, DiagKind::None) {
            return None;
        }

        let entering_active_diag = matches!(before_kind, DiagKind::None);
        let changed_active_diag = before_kind != after_kind
            || self.before.diag_targets != self.after.diag_targets
            || (matches!(self.before.phase, Phase::Initializing)
                && matches!(self.after.phase, Phase::DiagnosticsExclusive));

        if entering_active_diag || changed_active_diag {
            Some(AppStateDiagControl::Start {
                kind: after_kind,
                targets: self.after.diag_targets,
            })
        } else {
            None
        }
    }
}

pub(crate) struct AppStateEngine {
    machine: statig::blocking::StateMachine<AppStateMachine>,
}

impl AppStateEngine {
    pub(crate) fn new(snapshot: AppStateSnapshot) -> Self {
        Self {
            machine: AppStateMachine::new(snapshot).state_machine(),
        }
    }

    pub(crate) fn from_persisted(persisted: PersistedAppState) -> Self {
        let snapshot = AppStateSnapshot::from_persisted_sanitized(persisted);
        Self::new(snapshot)
    }

    pub(crate) fn snapshot(&self) -> AppStateSnapshot {
        self.machine.inner().snapshot
    }

    pub(crate) fn apply(&mut self, command: AppStateCommand) -> AppStateApplyResult {
        let before = self.snapshot();
        let mut context = DispatchContext::default();
        self.machine.handle_with_context(&command, &mut context);
        let after = self.snapshot();
        AppStateApplyResult {
            before,
            after,
            status: context.status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{events::AppStateCommand, types::*};
    use super::*;

    #[test]
    fn boot_transitions_initializing_to_operating() {
        let mut engine = AppStateEngine::new(AppStateSnapshot::default());
        let result = engine.apply(AppStateCommand::BootComplete);
        assert!(result.changed());
        assert!(matches!(result.after.phase, Phase::Operating));
    }

    #[test]
    fn touch_wizard_forces_overlay_none() {
        let mut snapshot = AppStateSnapshot::default();
        snapshot.phase = Phase::Operating;
        snapshot.overlay = OverlayMode::Clock;
        let mut engine = AppStateEngine::new(snapshot);
        let result = engine.apply(AppStateCommand::SetBase(BaseMode::TouchWizard));
        assert!(result.changed());
        assert!(matches!(result.after.overlay, OverlayMode::None));
    }

    #[test]
    fn clock_overlay_invalid_for_touch_wizard() {
        let mut snapshot = AppStateSnapshot::default();
        snapshot.phase = Phase::Operating;
        snapshot.base = BaseMode::TouchWizard;
        let mut engine = AppStateEngine::new(snapshot);
        let result = engine.apply(AppStateCommand::SetOverlay(OverlayMode::Clock));
        assert!(matches!(
            result.status,
            AppStateApplyStatus::InvalidTransition
        ));
    }

    #[test]
    fn boot_with_persisted_diag_emits_start_action() {
        let mut persisted = PersistedAppState::default();
        persisted.diag_kind = DiagKind::Test;
        persisted.diag_targets = DiagTargets::from_persisted(0b00011);
        let mut engine = AppStateEngine::from_persisted(persisted);
        let result = engine.apply(AppStateCommand::BootComplete);
        assert!(matches!(
            result.diag_control(),
            Some(AppStateDiagControl::Start {
                kind: DiagKind::Test,
                targets
            }) if targets.as_persisted() == 0b00011
        ));
    }

    #[test]
    fn clearing_diag_emits_stop_action() {
        let mut engine = AppStateEngine::new(AppStateSnapshot::default());
        let _ = engine.apply(AppStateCommand::BootComplete);
        let _ = engine.apply(AppStateCommand::SetDiag {
            kind: DiagKind::Debug,
            targets: DiagTargets::from_persisted(0b00001),
        });
        let clear = engine.apply(AppStateCommand::SetDiag {
            kind: DiagKind::None,
            targets: DiagTargets::none(),
        });
        assert!(matches!(
            clear.diag_control(),
            Some(AppStateDiagControl::Stop)
        ));
    }
}
