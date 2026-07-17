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

async fn power_on<E, P>(power: &mut P, mode: SdPowerMode) -> Result<(), E>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if matches!(mode, SdPowerMode::AlreadyOn) {
        return Ok(());
    }
    power_on_for_io(|| power(SdPowerAction::On)).await
}

fn power_off_io<E, P>(power: &mut P, mode: SdPowerMode) -> Result<(), E>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    if matches!(mode, SdPowerMode::AlreadyOn) {
        return Ok(());
    }
    power_off(|| power(SdPowerAction::Off))
}
