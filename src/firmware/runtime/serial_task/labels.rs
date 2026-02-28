use crate::firmware::types::{SdCommand, SdCommandKind, SdResultCode};

use super::commands::SdWaitTarget;

pub(super) fn sd_command_label(command: SdCommand) -> &'static str {
    match command {
        SdCommand::Probe => "probe",
        SdCommand::RwVerify { .. } => "rw_verify",
        SdCommand::FatList { .. } => "fat_ls",
        SdCommand::FatRead { .. } => "fat_read",
        SdCommand::FatWrite { .. } => "fat_write",
        SdCommand::FatStat { .. } => "fat_stat",
        SdCommand::FatMkdir { .. } => "fat_mkdir",
        SdCommand::FatRemove { .. } => "fat_rm",
        SdCommand::FatRename { .. } => "fat_ren",
        SdCommand::FatAppend { .. } => "fat_append",
        SdCommand::FatTruncate { .. } => "fat_trunc",
    }
}

pub(super) fn sd_result_kind_label(kind: SdCommandKind) -> &'static str {
    match kind {
        SdCommandKind::Probe => "probe",
        SdCommandKind::RwVerify => "rw_verify",
        SdCommandKind::FatList => "fat_ls",
        SdCommandKind::FatRead => "fat_read",
        SdCommandKind::FatWrite => "fat_write",
        SdCommandKind::FatStat => "fat_stat",
        SdCommandKind::FatMkdir => "fat_mkdir",
        SdCommandKind::FatRemove => "fat_rm",
        SdCommandKind::FatRename => "fat_ren",
        SdCommandKind::FatAppend => "fat_append",
        SdCommandKind::FatTruncate => "fat_trunc",
    }
}

pub(super) fn sdwait_target_label(target: SdWaitTarget) -> &'static str {
    match target {
        SdWaitTarget::Next => "next",
        SdWaitTarget::Last => "last",
        SdWaitTarget::Id(_) => "id",
    }
}

pub(super) fn sd_result_code_label(code: SdResultCode) -> &'static str {
    match code {
        SdResultCode::Ok => "ok",
        SdResultCode::PowerOnFailed => "power_on_failed",
        SdResultCode::InitFailed => "init_failed",
        SdResultCode::InvalidPath => "invalid_path",
        SdResultCode::NotFound => "not_found",
        SdResultCode::VerifyMismatch => "verify_mismatch",
        SdResultCode::PowerOffFailed => "power_off_failed",
        SdResultCode::OperationFailed => "operation_failed",
        SdResultCode::RefusedLba0 => "refused_lba0",
    }
}
