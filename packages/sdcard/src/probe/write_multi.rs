const SD_TOKEN_START_WRITE_MULTI: u8 = 0xFC;
const SD_TOKEN_STOP_WRITE_MULTI: u8 = 0xFD;
const SD_MULTI_WRITE_MAX_SECTORS: usize = 64;
const SD_WRITE_READY_POLL_LIMIT: usize = 200_000;

impl<'d> SdCardProbe<'d> {
    pub async fn write_sectors_contiguous(
        &mut self,
        start_lba: u32,
        data: &[u8],
    ) -> Result<(), SdProbeError> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() % SD_SECTOR_SIZE != 0 {
            return Err(SdProbeError::WriteLengthInvalid(data.len()));
        }

        let mut current_lba = start_lba;
        let mut byte_offset = 0usize;
        while byte_offset < data.len() {
            let sectors_left = (data.len() - byte_offset) / SD_SECTOR_SIZE;
            let burst_sectors = sectors_left.min(SD_MULTI_WRITE_MAX_SECTORS);
            let burst_bytes = burst_sectors * SD_SECTOR_SIZE;
            let burst = &data[byte_offset..byte_offset + burst_bytes];

            if burst_sectors >= 2 {
                // Keep CMD25 strictly opportunistic. Any anomaly drops to proven
                // CMD24-per-sector writes to preserve correctness and avoid
                // reintroducing storage-side instability while tuning throughput.
                if self.write_sectors_cmd25_burst(current_lba, burst).await.is_err() {
                    self.write_sectors_cmd24_fallback(current_lba, burst).await?;
                } else {
                    let last_lba = current_lba
                        .saturating_add((burst_sectors as u32).saturating_sub(1));
                    let last_sector = &burst[burst.len() - SD_SECTOR_SIZE..];
                    self.cached_sector.copy_from_slice(last_sector);
                    self.cached_sector_lba = Some(last_lba);
                }
            } else {
                self.write_sectors_cmd24_fallback(current_lba, burst).await?;
            }

            current_lba = current_lba
                .checked_add(burst_sectors as u32)
                .ok_or(SdProbeError::WriteLengthInvalid(data.len()))?;
            byte_offset += burst_bytes;
        }
        Ok(())
    }

    async fn write_sectors_cmd24_fallback(
        &mut self,
        start_lba: u32,
        data: &[u8],
    ) -> Result<(), SdProbeError> {
        let mut lba = start_lba;
        let mut sector = [0u8; SD_SECTOR_SIZE];
        for chunk in data.chunks_exact(SD_SECTOR_SIZE) {
            sector.copy_from_slice(chunk);
            self.write_sector(lba, &sector).await?;
            lba = lba
                .checked_add(1)
                .ok_or(SdProbeError::WriteLengthInvalid(data.len()))?;
        }
        Ok(())
    }

    async fn write_sectors_cmd25_burst(
        &mut self,
        start_lba: u32,
        data: &[u8],
    ) -> Result<(), SdProbeError> {
        let high_capacity = self.high_capacity.ok_or(SdProbeError::NotInitialized)?;
        let arg = if high_capacity {
            start_lba
        } else {
            start_lba.saturating_mul(SD_SECTOR_SIZE as u32)
        };

        let cmd25_r1 = self
            .send_command_hold_cs(SD_CMD25, arg, 0xFF, &mut [])
            .await?;
        if cmd25_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd25Unexpected(cmd25_r1));
        }

        for chunk in data.chunks_exact(SD_SECTOR_SIZE) {
            let _ = self.transfer_byte(0xFF).await?;
            let _ = self.transfer_byte(SD_TOKEN_START_WRITE_MULTI).await?;
            self.spi.write(chunk)?;
            let _ = self.transfer_byte(0xFF).await?;
            let _ = self.transfer_byte(0xFF).await?;

            let response = self.transfer_byte(0xFF).await? & 0x1F;
            if response != 0x05 {
                self.end_transaction().await;
                return Err(SdProbeError::WriteDataRejected(response));
            }
            if !self.wait_write_ready().await? {
                self.end_transaction().await;
                return Err(SdProbeError::WriteBusyTimeout);
            }
        }

        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(SD_TOKEN_STOP_WRITE_MULTI).await?;
        let _ = self.transfer_byte(0xFF).await?;
        let released = self.wait_write_ready().await?;
        self.end_transaction().await;
        if !released {
            return Err(SdProbeError::WriteBusyTimeout);
        }

        let mut status = [0xFFu8; 1];
        let cmd13_r1 = self.send_command(SD_CMD13, 0, 0xFF, &mut status).await?;
        if cmd13_r1 != 0x00 || status[0] != 0x00 {
            return Err(SdProbeError::Cmd13Unexpected(cmd13_r1, status[0]));
        }
        Ok(())
    }

    async fn wait_write_ready(&mut self) -> Result<bool, SdProbeError> {
        for _ in 0..SD_WRITE_READY_POLL_LIMIT {
            if self.transfer_byte(0xFF).await? == 0xFF {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
