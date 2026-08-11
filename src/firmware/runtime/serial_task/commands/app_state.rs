use super::types::SerialCommand;
use crate::firmware::app_state::AppStateCommand;

pub(in crate::firmware::runtime) fn app_state_command_for_serial(
    cmd: SerialCommand,
) -> Option<AppStateCommand> {
    match cmd {
        SerialCommand::StateSet { operation } => Some(operation.as_state_command()),
        SerialCommand::StateDiag { kind, targets } => {
            Some(AppStateCommand::SetDiag { kind, targets })
        }
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStart => Some(AppStateCommand::SetUpload(true)),
        #[cfg(feature = "asset-upload-http")]
        SerialCommand::NetStop => Some(AppStateCommand::SetUpload(false)),
        _ => None,
    }
}
