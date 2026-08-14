use super::probe_rw::SdPowerAction;
use crate::{power_off, power_on_for_io};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdRuntimeResultCode {
    Ok,
    PowerOnFailed,
    InitFailed,
    InvalidPath,
    NotFound,
    VerifyMismatch,
    PowerOffFailed,
    OperationFailed,
    RefusedLba0,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdPowerMode {
    Managed,
    AlreadyOn,
}

pub(super) async fn power_on<E, P>(power: &mut P, mode: SdPowerMode) -> Result<(), E>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if matches!(mode, SdPowerMode::AlreadyOn) {
        return Ok(());
    }
    power_on_for_io(|| power(SdPowerAction::On)).await
}

pub(super) fn power_off_io<E, P>(power: &mut P, mode: SdPowerMode) -> Result<(), E>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if matches!(mode, SdPowerMode::AlreadyOn) {
        return Ok(());
    }
    power_off(|| power(SdPowerAction::Off))
}
