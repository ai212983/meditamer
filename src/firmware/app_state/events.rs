use super::types::{BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AppStateCommand {
    BootComplete,
    SetBase(BaseMode),
    ToggleDayBackground,
    SetDayBackground(DayBackground),
    SetOverlay(OverlayMode),
    SetUpload(bool),
    SetAssets(bool),
    SetDiag {
        kind: DiagKind,
        targets: DiagTargets,
    },
}
