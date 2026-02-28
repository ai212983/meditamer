use statig::prelude::*;

use super::actions::AppStateApplyStatus;
use super::events::AppStateCommand;
use super::snapshot::AppStateSnapshot;
use super::types::{BaseMode, DiagKind, DiagTargets, OverlayMode, Phase};

#[derive(Clone, Copy, Debug)]
pub(super) struct AppStateMachine {
    pub(super) snapshot: AppStateSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DispatchContext {
    pub(super) status: AppStateApplyStatus,
}

impl Default for DispatchContext {
    fn default() -> Self {
        Self {
            status: AppStateApplyStatus::Unchanged,
        }
    }
}

impl AppStateMachine {
    pub(super) fn new(snapshot: AppStateSnapshot) -> Self {
        Self { snapshot }
    }

    fn apply_operating_command(&mut self, command: AppStateCommand) -> AppStateApplyStatus {
        let before = self.snapshot;
        match command {
            AppStateCommand::BootComplete => return AppStateApplyStatus::Unchanged,
            AppStateCommand::SetBase(base) => {
                self.snapshot.base = base;
                if matches!(base, BaseMode::TouchWizard) {
                    self.snapshot.overlay = OverlayMode::None;
                }
            }
            AppStateCommand::ToggleDayBackground => {
                if !matches!(self.snapshot.base, BaseMode::Day) {
                    return AppStateApplyStatus::InvalidTransition;
                }
                self.snapshot.day_background = self.snapshot.day_background.toggled();
            }
            AppStateCommand::SetDayBackground(day_background) => {
                self.snapshot.day_background = day_background;
            }
            AppStateCommand::SetOverlay(overlay) => {
                if matches!(overlay, OverlayMode::Clock)
                    && !matches!(self.snapshot.base, BaseMode::Day)
                {
                    return AppStateApplyStatus::InvalidTransition;
                }
                self.snapshot.overlay = overlay;
            }
            AppStateCommand::SetUpload(enabled) => {
                self.snapshot.services.upload_enabled = enabled;
            }
            AppStateCommand::SetAssets(enabled) => {
                self.snapshot.services.asset_reads_enabled = enabled;
            }
            AppStateCommand::SetDiag { kind, targets } => {
                self.snapshot.diag_kind = kind;
                self.snapshot.diag_targets = if matches!(kind, DiagKind::None) {
                    DiagTargets::none()
                } else {
                    targets
                };
            }
        }

        if before == self.snapshot {
            AppStateApplyStatus::Unchanged
        } else {
            AppStateApplyStatus::Applied
        }
    }
}

#[state_machine(initial = "State::initializing()")]
impl AppStateMachine {
    #[state]
    fn initializing(
        &mut self,
        context: &mut DispatchContext,
        event: &AppStateCommand,
    ) -> Outcome<State> {
        match event {
            AppStateCommand::BootComplete => {
                let target_phase = if matches!(self.snapshot.diag_kind, DiagKind::None) {
                    Phase::Operating
                } else {
                    Phase::DiagnosticsExclusive
                };
                if self.snapshot.phase != target_phase {
                    self.snapshot.phase = target_phase;
                    context.status = AppStateApplyStatus::Applied;
                } else {
                    context.status = AppStateApplyStatus::Unchanged;
                }
                if matches!(target_phase, Phase::DiagnosticsExclusive) {
                    Transition(State::diagnostics_exclusive())
                } else {
                    Transition(State::operating())
                }
            }
            _ => {
                context.status = AppStateApplyStatus::InvalidTransition;
                Handled
            }
        }
    }

    #[state]
    fn operating(
        &mut self,
        context: &mut DispatchContext,
        event: &AppStateCommand,
    ) -> Outcome<State> {
        context.status = self.apply_operating_command(*event);
        if matches!(self.snapshot.diag_kind, DiagKind::Debug | DiagKind::Test) {
            self.snapshot.phase = Phase::DiagnosticsExclusive;
            return Transition(State::diagnostics_exclusive());
        }
        self.snapshot.phase = Phase::Operating;
        Handled
    }

    #[state]
    fn diagnostics_exclusive(
        &mut self,
        context: &mut DispatchContext,
        event: &AppStateCommand,
    ) -> Outcome<State> {
        context.status = self.apply_operating_command(*event);
        if matches!(self.snapshot.diag_kind, DiagKind::None) {
            self.snapshot.phase = Phase::Operating;
            return Transition(State::operating());
        }
        self.snapshot.phase = Phase::DiagnosticsExclusive;
        Handled
    }
}
