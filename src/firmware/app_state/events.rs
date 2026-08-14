use super::types::{DiagKind, DiagTargets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AppStateCommand {
    BootComplete,
    SetUpload(bool),
    SetDiag {
        kind: DiagKind,
        targets: DiagTargets,
    },
}
