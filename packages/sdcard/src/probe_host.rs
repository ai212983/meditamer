use core::marker::PhantomData;

pub const SD_SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SdWriteMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdProbeError {
    HostStub,
}

pub struct SdCardProbe<'d> {
    _lifetime: PhantomData<&'d mut ()>,
}

impl SdCardProbe<'_> {
    pub async fn read_sector(
        &mut self,
        _lba: u32,
        _out: &mut [u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        Err(SdProbeError::HostStub)
    }

    pub async fn write_sector(
        &mut self,
        _lba: u32,
        _data: &[u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        Err(SdProbeError::HostStub)
    }

    pub async fn write_sectors_contiguous(
        &mut self,
        _lba: u32,
        _data: &[u8],
    ) -> Result<(), SdProbeError> {
        Err(SdProbeError::HostStub)
    }
}
