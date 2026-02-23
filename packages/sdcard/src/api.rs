use crate::{
    power_off, power_on_for_io,
    fat::{self, FatDirEntry},
    probe::{self, SdCardProbe, SdProbeStatus, SD_SECTOR_SIZE},
};

#[derive(Debug)]
pub enum SdIoError<E> {
    Power(E),
    Probe(probe::SdProbeError),
    Fat(fat::SdFatError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdPowerAction {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RwVerifyResult {
    pub lba: u32,
    pub bytes: usize,
    pub mismatch_index: Option<usize>,
    pub before: u8,
    pub after: u8,
}

impl<E> From<probe::SdProbeError> for SdIoError<E> {
    fn from(value: probe::SdProbeError) -> Self {
        Self::Probe(value)
    }
}

impl<E> From<fat::SdFatError> for SdIoError<E> {
    fn from(value: fat::SdFatError) -> Self {
        Self::Fat(value)
    }
}

async fn power_on<E, P>(power: &mut P) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on_for_io(|| power(SdPowerAction::On))
        .await
        .map_err(SdIoError::Power)
}

fn power_off_io<E, P>(power: &mut P) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_off(|| power(SdPowerAction::Off)).map_err(SdIoError::Power)
}

pub async fn probe<E, P>(
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<SdProbeStatus, SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = sd_probe.probe().await.map_err(SdIoError::Probe);
    let _ = power_off_io(power);
    result
}

pub async fn rw_verify<E, P>(
    lba: u32,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<RwVerifyResult, SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        let mut before = [0u8; SD_SECTOR_SIZE];
        sd_probe
            .read_sector(lba, &mut before)
            .await
            .map_err(SdIoError::Probe)?;
        sd_probe
            .write_sector(lba, &before)
            .await
            .map_err(SdIoError::Probe)?;
        let mut after = [0u8; SD_SECTOR_SIZE];
        sd_probe
            .read_sector(lba, &mut after)
            .await
            .map_err(SdIoError::Probe)?;
        if let Some(idx) = before.iter().zip(after.iter()).position(|(a, b)| a != b) {
            Ok(RwVerifyResult {
                lba,
                bytes: SD_SECTOR_SIZE,
                mismatch_index: Some(idx),
                before: before[idx],
                after: after[idx],
            })
        } else {
            Ok(RwVerifyResult {
                lba,
                bytes: SD_SECTOR_SIZE,
                mismatch_index: None,
                before: 0,
                after: 0,
            })
        }
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_list<E, P>(
    path: &str,
    out: &mut [FatDirEntry],
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<usize, SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::list_dir(sd_probe, path, out)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_read<E, P>(
    path: &str,
    out: &mut [u8],
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<usize, SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::read_file(sd_probe, path, out)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_write<E, P>(
    path: &str,
    data: &[u8],
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::write_file(sd_probe, path, data)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_stat<E, P>(
    path: &str,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<FatDirEntry, SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::stat(sd_probe, path).await.map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_mkdir<E, P>(
    path: &str,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::mkdir(sd_probe, path).await.map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_remove<E, P>(
    path: &str,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::remove(sd_probe, path).await.map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_rename<E, P>(
    src: &str,
    dst: &str,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::rename(sd_probe, src, dst)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_append<E, P>(
    path: &str,
    data: &[u8],
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::append_file(sd_probe, path, data)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}

pub async fn fat_truncate<E, P>(
    path: &str,
    new_size: usize,
    sd_probe: &mut SdCardProbe<'_>,
    power: &mut P,
) -> Result<(), SdIoError<E>>
where
    P: FnMut(SdPowerAction) -> Result<(), E>,
{
    power_on(power).await?;
    let result = async {
        sd_probe.init().await.map_err(SdIoError::Probe)?;
        fat::truncate_file(sd_probe, path, new_size)
            .await
            .map_err(SdIoError::Fat)
    }
    .await;
    let _ = power_off_io(power);
    result
}
