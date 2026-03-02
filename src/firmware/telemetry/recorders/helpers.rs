fn saturating_add_u32(counter: &AtomicU32, value: u32) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_add(value))
    });
}

fn update_max_u32(max_counter: &AtomicU32, value: u32) {
    let mut current = max_counter.load(Ordering::Relaxed);
    while value > current {
        match max_counter.compare_exchange_weak(
            current,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}

#[cfg(feature = "telemetry-defmt")]
fn sd_upload_result_code_to_u8(code: SdUploadResultCode) -> u8 {
    match code {
        SdUploadResultCode::Ok => 0,
        SdUploadResultCode::Busy => 1,
        SdUploadResultCode::SessionNotActive => 2,
        SdUploadResultCode::InvalidPath => 3,
        SdUploadResultCode::NotFound => 4,
        SdUploadResultCode::NotEmpty => 5,
        SdUploadResultCode::SizeMismatch => 6,
        SdUploadResultCode::PowerOnFailed => 7,
        SdUploadResultCode::InitFailed => 8,
        SdUploadResultCode::DirectoryFull => 9,
        SdUploadResultCode::OperationFailed => 10,
    }
}
