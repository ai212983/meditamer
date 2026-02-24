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

fn decode_path(buf: &[u8], len: u8) -> Option<&str> {
    let len = len as usize;
    if len == 0 || len > buf.len() {
        return None;
    }
    core::str::from_utf8(&buf[..len]).ok()
}

fn decode_entry_name(entry: &fat::FatDirEntry) -> &str {
    let len = entry.name_len as usize;
    if len == 0 || len > entry.name.len() {
        return "<invalid>";
    }
    core::str::from_utf8(&entry.name[..len]).unwrap_or("<utf8_err>")
}

fn fat_error_result_code(error: &fat::SdFatError) -> SdRuntimeResultCode {
    match error {
        fat::SdFatError::InvalidPath => SdRuntimeResultCode::InvalidPath,
        fat::SdFatError::NotFound => SdRuntimeResultCode::NotFound,
        _ => SdRuntimeResultCode::OperationFailed,
    }
}
