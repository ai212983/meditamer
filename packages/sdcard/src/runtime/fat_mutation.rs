pub async fn run_sd_fat_stat<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: stat invalid_path", reason);
            return;
        }
    };
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: stat power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: stat init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::stat(sd_probe, path).await {
        Ok(meta) => {
            let kind = if meta.is_dir { "dir" } else { "file" };
            let name = core::str::from_utf8(&meta.name[..meta.name_len as usize]).unwrap_or(path);
            esp_println::println!(
                "sdfat[{}]: stat_ok path={} kind={} name={} size={}",
                reason,
                path,
                kind,
                name,
                meta.size
            );
        }
        Err(err) => {
            esp_println::println!("sdfat[{}]: stat_error path={} err={:?}", reason, path, err);
        }
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: stat power_off_error", reason);
    }
}

pub async fn run_sd_fat_mkdir<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: mkdir invalid_path", reason);
            return;
        }
    };
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: mkdir power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: mkdir init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::mkdir(sd_probe, path).await {
        Ok(()) => esp_println::println!("sdfat[{}]: mkdir_ok path={}", reason, path),
        Err(err) => esp_println::println!("sdfat[{}]: mkdir_error path={} err={:?}", reason, path, err),
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: mkdir power_off_error", reason);
    }
}

pub async fn run_sd_fat_remove<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: rm invalid_path", reason);
            return;
        }
    };
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: rm power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: rm init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::remove(sd_probe, path).await {
        Ok(()) => esp_println::println!("sdfat[{}]: rm_ok path={}", reason, path),
        Err(err) => esp_println::println!("sdfat[{}]: rm_error path={} err={:?}", reason, path, err),
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: rm power_off_error", reason);
    }
}

pub async fn run_sd_fat_rename<E, P>(
    reason: &str,
    src_path_buf: &[u8],
    src_path_len: u8,
    dst_path_buf: &[u8],
    dst_path_len: u8,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let src_path = match decode_path(src_path_buf, src_path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: ren invalid_src_path", reason);
            return;
        }
    };
    let dst_path = match decode_path(dst_path_buf, dst_path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: ren invalid_dst_path", reason);
            return;
        }
    };
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: ren power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: ren init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::rename(sd_probe, src_path, dst_path).await {
        Ok(()) => esp_println::println!("sdfat[{}]: ren_ok src={} dst={}", reason, src_path, dst_path),
        Err(err) => esp_println::println!(
            "sdfat[{}]: ren_error src={} dst={} err={:?}",
            reason,
            src_path,
            dst_path,
            err
        ),
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: ren power_off_error", reason);
    }
}

pub async fn run_sd_fat_append<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    data_buf: &[u8],
    data_len: u16,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: append invalid_path", reason);
            return;
        }
    };
    let data_len = core::cmp::min(data_len as usize, data_buf.len());
    let data = &data_buf[..data_len];
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: append power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: append init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::append_file(sd_probe, path, data).await {
        Ok(()) => esp_println::println!(
            "sdfat[{}]: append_ok path={} bytes={}",
            reason,
            path,
            data.len()
        ),
        Err(err) => esp_println::println!(
            "sdfat[{}]: append_error path={} bytes={} err={:?}",
            reason,
            path,
            data.len(),
            err
        ),
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: append power_off_error", reason);
    }
}

pub async fn run_sd_fat_truncate<E, P>(
    reason: &str,
    path_buf: &[u8],
    path_len: u8,
    size: u32,
    sd_probe: &mut probe::SdCardProbe<'_>,
    power: &mut P,
) where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    let path = match decode_path(path_buf, path_len) {
        Some(path) => path,
        None => {
            esp_println::println!("sdfat[{}]: trunc invalid_path", reason);
            return;
        }
    };
    if power_on(power).await.is_err() {
        esp_println::println!("sdfat[{}]: trunc power_on_error", reason);
        return;
    }
    if let Err(err) = sd_probe.init().await {
        esp_println::println!("sdfat[{}]: trunc init_error={:?}", reason, err);
        let _ = power_off_io(power);
        return;
    }
    match fat::truncate_file(sd_probe, path, size as usize).await {
        Ok(()) => esp_println::println!("sdfat[{}]: trunc_ok path={} size={}", reason, path, size),
        Err(err) => esp_println::println!(
            "sdfat[{}]: trunc_error path={} size={} err={:?}",
            reason,
            path,
            size,
            err
        ),
    }
    if power_off_io(power).is_err() {
        esp_println::println!("sdfat[{}]: trunc power_off_error", reason);
    }
}

