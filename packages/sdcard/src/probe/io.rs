impl<'d, SPI> SdCardProbe<'d, SPI>
where
    SPI: SdSpiBus,
{
    pub async fn read_sector(
        &mut self,
        lba: u32,
        out: &mut [u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        if self.cached_sector_lba == Some(lba) {
            out.copy_from_slice(&self.cached_sector[..SD_SECTOR_SIZE]);
            return Ok(());
        }
        let high_capacity = self.high_capacity.ok_or(SdProbeError::NotInitialized)?;
        self.read_data_sector_512_into(lba, high_capacity, out).await?;
        self.cached_sector[..SD_SECTOR_SIZE].copy_from_slice(out);
        self.cached_sector_lba = Some(lba);
        Ok(())
    }

    pub async fn write_sector(
        &mut self,
        lba: u32,
        data: &[u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        let high_capacity = self.high_capacity.ok_or(SdProbeError::NotInitialized)?;
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(SD_SECTOR_SIZE as u32)
        };

        let cmd24_r1 = self
            .send_command_hold_cs(SD_CMD24, arg, 0xFF, &mut [])
            .await?;
        if cmd24_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd24Unexpected(cmd24_r1));
        }

        let (response, ready) = self.transfer_write_frame(0xFE, data).await?;
        if response != 0x05 {
            self.end_transaction().await;
            return Err(SdProbeError::WriteDataRejected(response));
        }
        self.end_transaction().await;
        if !ready.released {
            return Err(SdProbeError::WriteBusyTimeout {
                elapsed_ms: ready.elapsed_ms,
                polls: ready.polls,
            });
        }
        self.record_cmd24_sector_write();
        self.cached_sector[..SD_SECTOR_SIZE].copy_from_slice(data);
        self.cached_sector_lba = Some(lba);
        Ok(())
    }

    async fn send_command(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, true)
            .await
    }

    async fn send_command_hold_cs(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, false)
            .await
    }

    async fn send_command_inner(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
        release_cs_after: bool,
    ) -> Result<u8, SdProbeError> {
        let mut frame = [
            0x40 | cmd,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            crc,
        ];

        self.cs.set_low();
        self.spi
            .transfer_in_place_with_deadline(&mut frame)
            .await?;

        let mut r1 = 0xFFu8;
        let mut got_response = false;
        for _ in 0..16 {
            r1 = self.transfer_byte(0xFF).await?;
            if (r1 & 0x80) == 0 {
                got_response = true;
                break;
            }
            embassy_futures::yield_now().await;
        }

        if !got_response {
            self.end_transaction().await;
            return Err(SdProbeError::NoResponse(cmd));
        }

        if !extra_response.is_empty() {
            extra_response.fill(0xFF);
            self.spi
                .transfer_in_place_with_deadline(extra_response)
                .await?;
        }

        if release_cs_after {
            self.end_transaction().await;
        }
        Ok(r1)
    }

    async fn send_dummy_clocks(&mut self, bytes: usize) -> Result<(), SdProbeError> {
        const DUMMY_CLOCK_CHUNK: usize = 32;
        let mut frame = [0xFFu8; DUMMY_CLOCK_CHUNK];
        let mut remaining = bytes;
        while remaining > 0 {
            let chunk = remaining.min(DUMMY_CLOCK_CHUNK);
            self.spi
                .transfer_in_place_with_deadline(&mut frame[..chunk])
                .await?;
            remaining -= chunk;
        }
        Ok(())
    }

    async fn transfer_byte(&mut self, byte: u8) -> Result<u8, SdProbeError> {
        let mut frame = [byte];
        self.spi
            .transfer_in_place_with_deadline(&mut frame)
            .await?;
        Ok(frame[0])
    }

    async fn read_data_block(&mut self) -> Result<[u8; 16], SdProbeError> {
        let token = match self.wait_data_token(SD_CMD9).await {
            Ok(token) => token,
            Err(err) => {
                self.end_transaction().await;
                return Err(err);
            }
        };
        if token != 0xFE {
            self.end_transaction().await;
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD9, token));
        }

        let mut block = [0xFFu8; 16];
        self.spi
            .transfer_in_place_with_deadline(&mut block)
            .await?;
        // Read and discard CRC16.
        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFF).await?;
        Ok(block)
    }

    async fn read_data_sector_512_into(
        &mut self,
        lba: u32,
        high_capacity: bool,
        out: &mut [u8; SD_SECTOR_SIZE],
    ) -> Result<(), SdProbeError> {
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(512)
        };
        let cmd17_r1 = self
            .send_command_hold_cs(SD_CMD17, arg, 0xFF, &mut [])
            .await?;
        if cmd17_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd17Unexpected(cmd17_r1));
        }

        let token = match self.wait_data_token(SD_CMD17).await {
            Ok(token) => token,
            Err(err) => {
                self.end_transaction().await;
                return Err(err);
            }
        };
        if token != 0xFE {
            self.end_transaction().await;
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD17, token));
        }

        out.fill(0xFF);
        self.spi.transfer_in_place_with_deadline(out).await?;
        // Discard data CRC16.
        let _ = self.transfer_byte(0xFF).await?;
        let _ = self.transfer_byte(0xFF).await?;
        self.end_transaction().await;
        Ok(())
    }

    async fn end_transaction(&mut self) {
        self.cs.set_high();
        let _ = self.transfer_byte(0xFF).await;
    }
}
