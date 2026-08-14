use super::wait::elapsed_ms_u32;
use super::{
    SdCardProbe, SdProbeError, SdSpiBus, SD_ACMD23, SD_CMD13, SD_CMD25, SD_CMD55, SD_SECTOR_SIZE,
};
use embassy_time::Instant;

const SD_TOKEN_START_WRITE_MULTI: u8 = 0xFC;
const SD_TOKEN_STOP_WRITE_MULTI: u8 = 0xFD;
const SD_MULTI_WRITE_MAX_SECTORS: usize = 64;

impl<'d, SPI> SdCardProbe<'d, SPI>
where
    SPI: SdSpiBus,
{
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
                self.record_cmd25_attempt(burst_sectors);
                let burst_started_at = Instant::now();
                // Keep CMD25 strictly opportunistic. Any anomaly drops to proven
                // CMD24-per-sector writes to preserve correctness and avoid
                // reintroducing storage-side instability while tuning throughput.
                if self
                    .write_sectors_cmd25_burst(current_lba, burst)
                    .await
                    .is_err()
                {
                    self.record_cmd25_fallback();
                    self.write_sectors_cmd24_fallback(current_lba, burst)
                        .await?;
                } else {
                    let burst_elapsed_ms = elapsed_ms_u32(burst_started_at);
                    self.record_cmd25_success(burst_sectors, burst_elapsed_ms);
                    let last_lba =
                        current_lba.saturating_add((burst_sectors as u32).saturating_sub(1));
                    let last_sector = &burst[burst.len() - SD_SECTOR_SIZE..];
                    self.cached_sector[..SD_SECTOR_SIZE].copy_from_slice(last_sector);
                    self.cached_sector_lba = Some(last_lba);
                }
            } else {
                self.write_sectors_cmd24_fallback(current_lba, burst)
                    .await?;
            }

            current_lba = current_lba
                .checked_add(burst_sectors as u32)
                .ok_or(SdProbeError::WriteLengthInvalid(data.len()))?;
            byte_offset += burst_bytes;
            embassy_futures::yield_now().await;
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
            embassy_futures::yield_now().await;
        }
        Ok(())
    }

    async fn write_sectors_cmd25_burst(
        &mut self,
        start_lba: u32,
        data: &[u8],
    ) -> Result<(), SdProbeError> {
        self.send_preerase_hint((data.len() / SD_SECTOR_SIZE) as u32)
            .await?;
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
            let (response, ready) = self
                .transfer_write_frame(SD_TOKEN_START_WRITE_MULTI, chunk)
                .await?;
            if response != 0x05 {
                self.end_transaction().await;
                return Err(SdProbeError::WriteDataRejected(response));
            }
            self.record_cmd25_ready_wait(ready.elapsed_ms, ready.polls);
            if !ready.released {
                self.end_transaction().await;
                return Err(SdProbeError::WriteBusyTimeout {
                    elapsed_ms: ready.elapsed_ms,
                    polls: ready.polls,
                });
            }
            // A CMD25 burst is one protocol transaction but must not be one
            // cooperative-executor timeslice. Give higher-priority input work
            // a scheduling boundary after every accepted sector while CS stays
            // asserted and the card remains in multi-block write mode.
            embassy_futures::yield_now().await;
        }

        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(SD_TOKEN_STOP_WRITE_MULTI).await?;
        let _ = self.transfer_byte(0xFF).await?;
        let stop_ready = self.wait_write_ready_timed().await?;
        self.record_cmd25_ready_wait(stop_ready.elapsed_ms, stop_ready.polls);
        let released = stop_ready.released;
        self.end_transaction().await;
        if !released {
            return Err(SdProbeError::WriteBusyTimeout {
                elapsed_ms: stop_ready.elapsed_ms,
                polls: stop_ready.polls,
            });
        }

        let mut status = [0xFFu8; 1];
        let cmd13_r1 = self.send_command(SD_CMD13, 0, 0xFF, &mut status).await?;
        if cmd13_r1 != 0x00 || status[0] != 0x00 {
            return Err(SdProbeError::Cmd13Unexpected(cmd13_r1, status[0]));
        }
        Ok(())
    }

    async fn send_preerase_hint(&mut self, sectors: u32) -> Result<(), SdProbeError> {
        self.record_acmd23_attempt();
        let app_r1 = self.send_command(SD_CMD55, 0, 0xFF, &mut []).await?;
        if app_r1 != 0x00 {
            self.record_acmd23_result(false);
            return Ok(());
        }
        let preerase_r1 = self.send_command(SD_ACMD23, sectors, 0xFF, &mut []).await?;
        self.record_acmd23_result(preerase_r1 == 0x00);
        Ok(())
    }
}
