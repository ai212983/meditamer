use super::{
    crc::{crc16, validate_and_publish},
    SdCardProbe, SdProbeError, SdSpiBus, SD_CMD17, SD_CMD24, SD_CMD9, SD_SECTOR_SIZE,
};

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
        // Retire the old tag before DMA mutates the shared frame so a failed
        // cache miss cannot expose partial data as the previous LBA.
        self.cached_sector_lba = None;
        // FAT state may live in PSRAM, which is not a valid ESP32 SPI DMA
        // target. Always transfer through the probe-owned internal frame.
        let wire_crc = self
            .read_data_sector_512_into_cached(lba, high_capacity)
            .await?;
        validate_and_publish(
            &self.cached_sector[..SD_SECTOR_SIZE],
            wire_crc,
            out,
            &mut self.cached_sector_lba,
            lba,
        )
        .map_err(|mismatch| SdProbeError::ReadCrcMismatch {
            command: SD_CMD17,
            expected: mismatch.expected,
            received: mismatch.received,
        })?;
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

    pub(super) async fn send_command(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, true)
            .await
    }

    pub(super) async fn send_command_hold_cs(
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
        self.transfer_init_bounded(&mut frame).await?;

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
            self.transfer_init_bounded(extra_response).await?;
        }

        if release_cs_after {
            self.end_transaction().await;
        }
        Ok(r1)
    }

    pub(super) async fn send_dummy_clocks(&mut self, bytes: usize) -> Result<(), SdProbeError> {
        const DUMMY_CLOCK_CHUNK: usize = 32;
        let mut frame = [0xFFu8; DUMMY_CLOCK_CHUNK];
        let mut remaining = bytes;
        while remaining > 0 {
            let chunk = remaining.min(DUMMY_CLOCK_CHUNK);
            self.transfer_init_bounded(&mut frame[..chunk]).await?;
            remaining -= chunk;
        }
        Ok(())
    }

    pub(super) async fn transfer_byte(&mut self, byte: u8) -> Result<u8, SdProbeError> {
        let mut frame = [byte];
        self.transfer_init_bounded(&mut frame).await?;
        Ok(frame[0])
    }

    pub(super) async fn read_data_block(&mut self) -> Result<[u8; 16], SdProbeError> {
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
        if let Err(err) = self.transfer_init_bounded(&mut block).await {
            self.end_transaction().await;
            return Err(err);
        }
        let wire_crc = match self.read_wire_crc().await {
            Ok(crc) => crc,
            Err(err) => {
                self.end_transaction().await;
                return Err(err);
            }
        };
        self.end_transaction().await;
        let expected = crc16(&block);
        if wire_crc != expected {
            return Err(SdProbeError::ReadCrcMismatch {
                command: SD_CMD9,
                expected,
                received: wire_crc,
            });
        }
        Ok(block)
    }

    pub(super) async fn read_data_sector_512_into(
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
        if let Err(err) = self.transfer_init_bounded(out).await {
            self.end_transaction().await;
            return Err(err);
        }
        let wire_crc = match self.read_wire_crc().await {
            Ok(crc) => crc,
            Err(err) => {
                self.end_transaction().await;
                return Err(err);
            }
        };
        self.end_transaction().await;
        let expected = crc16(out);
        if wire_crc != expected {
            return Err(SdProbeError::ReadCrcMismatch {
                command: SD_CMD17,
                expected,
                received: wire_crc,
            });
        }
        Ok(())
    }

    async fn read_data_sector_512_into_cached(
        &mut self,
        lba: u32,
        high_capacity: bool,
    ) -> Result<u16, SdProbeError> {
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(SD_SECTOR_SIZE as u32)
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

        self.cached_sector[..SD_SECTOR_SIZE].fill(0xFF);
        self.check_init_deadline()?;
        if let Err(err) = self
            .spi
            .transfer_in_place_to_completion(&mut self.cached_sector[..SD_SECTOR_SIZE])
            .await
        {
            self.end_transaction().await;
            return Err(err);
        }
        if let Err(err) = self.check_init_deadline() {
            self.end_transaction().await;
            return Err(err);
        }
        let wire_crc = match self.read_wire_crc().await {
            Ok(crc) => crc,
            Err(err) => {
                self.end_transaction().await;
                return Err(err);
            }
        };
        self.end_transaction().await;
        Ok(wire_crc)
    }

    async fn read_wire_crc(&mut self) -> Result<u16, SdProbeError> {
        let high = self.transfer_byte(0xFF).await?;
        let low = self.transfer_byte(0xFF).await?;
        Ok(u16::from_be_bytes([high, low]))
    }

    pub(super) async fn end_transaction(&mut self) {
        self.cs.set_high();
        let _ = self.transfer_byte(0xFF).await;
    }
}
