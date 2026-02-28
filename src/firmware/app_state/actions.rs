use super::types::{DiagKind, DiagTargets};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AppStateApplyStatus {
    Applied,
    Unchanged,
    InvalidTransition,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AppStateDiagControl {
    Start {
        kind: DiagKind,
        targets: DiagTargets,
    },
    Stop,
}
