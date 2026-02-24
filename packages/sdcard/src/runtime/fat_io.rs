pub async fn run_sd_fat_ls<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) -> SdRuntimeResultCode
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: ls invalid_path", reason);
            return SdRuntimeResultCode::InvalidPath;
        }
    };

    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: ls power_on_error", reason);
        return SdRuntimeResultCode::PowerOnFailed;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: ls init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return SdRuntimeResultCode::InitFailed;
    }

    let mut code = SdRuntimeResultCode::Ok;
    let mut entries = [fat::FatDirEntry::EMPTY; 32];
    match fat::list_dir(sd_probe, path, &mut entries).await {
        Ok(count) => {
            esp_println::println!("sdfat[{}]: ls_ok path={} count={}", reason, path, count);
            for entry in entries.iter().take(count) {
                let name = decode_entry_name(entry);
                let kind = if entry.is_dir { "dir" } else { "file" };
                esp_println::println!(
                    "sdfat[{}]: ls {} name={} size={}",
                    reason,
                    kind,
                    name,
                    entry.size
                );
            }
        }
        Err(err) => {
            esp_println::println!("sdfat[{}]: ls_error path={} err={:?}", reason, path, err);
            code = fat_error_result_code(&err);
        }
    }

    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: ls power_off_error", reason);
        return SdRuntimeResultCode::PowerOffFailed;
    }
    code
}

pub async fn run_sd_fat_read<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) -> SdRuntimeResultCode
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: read invalid_path", reason);
            return SdRuntimeResultCode::InvalidPath;
        }
    };

    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: read power_on_error", reason);
        return SdRuntimeResultCode::PowerOnFailed;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: read init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return SdRuntimeResultCode::InitFailed;
    }

    let mut code = SdRuntimeResultCode::Ok;
    let mut data = [0u8; 256];
    match fat::read_file(sd_probe, path, &mut data).await {
        Ok(size) => {
            let preview_len = core::cmp::min(size, 64);
            let mut hex = heapless::String::<196>::new();
            for byte in data.iter().take(preview_len) {
                let _ = write!(&mut hex, "{:02x}", byte);
            }
            esp_println::println!(
                "sdfat[{}]: read_ok path={} bytes={} preview_hex={}",
                reason,
                path,
                size,
                hex
            );
        }
        Err(fat::SdFatError::BufferTooSmall { needed }) => {
            esp_println::println!(
                "sdfat[{}]: read_error path={} err=buffer_too_small needed={}",
                reason,
                path,
                needed
            );
            code = SdRuntimeResultCode::OperationFailed;
        }
        Err(err) => {
            esp_println::println!("sdfat[{}]: read_error path={} err={:?}", reason, path, err);
            code = fat_error_result_code(&err);
        }
    }

    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: read power_off_error", reason);
        return SdRuntimeResultCode::PowerOffFailed;
    }
    code
}

pub async fn run_sd_fat_write<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    data_buf: &[u8],
    data_len: u16,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) -> SdRuntimeResultCode
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: write invalid_path", reason);
            return SdRuntimeResultCode::InvalidPath;
        }
    };

    let data_len = core::cmp::min(data_len as usize, data_buf.len());
    let data = &data_buf[..data_len];

    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: write power_on_error", reason);
        return SdRuntimeResultCode::PowerOnFailed;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: write init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return SdRuntimeResultCode::InitFailed;
    }

    let mut code = SdRuntimeResultCode::Ok;
    match fat::write_file(sd_probe, path, data).await {
        Ok(()) => {
            let mut verify = [0u8; SD_WRITE_MAX];
            let read_result = fat::read_file(sd_probe, path, &mut verify).await;
            match read_result {
                Ok(read_len) => {
                    let cmp_len = core::cmp::min(read_len, data.len());
                    let ok = cmp_len == data.len() && verify[..cmp_len] == data[..cmp_len];
                    esp_println::println!(
                        "sdfat[{}]: write_ok path={} bytes={} verify={}",
                        reason,
                        path,
                        data.len(),
                        if ok { "ok" } else { "mismatch" }
                    );
                    if !ok {
                        code = SdRuntimeResultCode::VerifyMismatch;
                    }
                }
                Err(err) => {
                    esp_println::println!(
                        "sdfat[{}]: write_ok path={} bytes={} verify_read_err={:?}",
                        reason,
                        path,
                        data.len(),
                        err
                    );
                    code = SdRuntimeResultCode::OperationFailed;
                }
            }
        }
        Err(err) => {
            esp_println::println!("sdfat[{}]: write_error path={} err={:?}", reason, path, err);
            code = fat_error_result_code(&err);
        }
    }

    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: write power_off_error", reason);
        return SdRuntimeResultCode::PowerOffFailed;
    }
    code
}
