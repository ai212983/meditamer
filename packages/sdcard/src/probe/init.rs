impl<'d> SdCardProbe<'d> {
    pub async fn init(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.cached_sector_lba = None;
        self.probe().await
    }

    pub async fn probe(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.apply_init_clock().await?;
        self.cs.set_high();
        self.send_dummy_clocks(10).await?;

        let mut cmd0_r1 = 0xFFu8;
        for _ in 0..16 {
            cmd0_r1 = self.send_command(SD_CMD0, 0, 0x95, &mut []).await?;
            if cmd0_r1 == 0x01 {
                break;
            }
        }
        if cmd0_r1 != 0x01 {
            return Err(SdProbeError::Cmd0Failed(cmd0_r1));
        }

        let mut r7 = [0u8; 4];
        let cmd8_r1 = self
            .send_command(SD_CMD8, 0x0000_01AA, 0x87, &mut r7)
            .await?;
        let card_version = if cmd8_r1 == 0x01 {
            if r7[2] != 0x01 || r7[3] != 0xAA {
                return Err(SdProbeError::Cmd8EchoMismatch(r7));
            }
            SdCardVersion::V2
        } else if (cmd8_r1 & 0x04) != 0 {
            SdCardVersion::V1
        } else {
            return Err(SdProbeError::Cmd8Unexpected(cmd8_r1));
        };

        let acmd41_arg = if card_version == SdCardVersion::V2 {
            0x4000_0000
        } else {
            0
        };
        let mut acmd41_r1 = 0xFFu8;
        let mut acmd41_ok = false;
        for _ in 0..200 {
            let _ = self.send_command(SD_CMD55, 0, 0x65, &mut []).await?;
            acmd41_r1 = self
                .send_command(SD_ACMD41, acmd41_arg, 0x77, &mut [])
                .await?;
            if acmd41_r1 == 0x00 {
                acmd41_ok = true;
                break;
            }
            self.retry_delay().await;
        }
        if !acmd41_ok {
            return Err(SdProbeError::Acmd41Timeout(acmd41_r1));
        }

        if card_version == SdCardVersion::V1 {
            let cmd16_r1 = self
                .send_command(SD_CMD16, SD_SECTOR_SIZE as u32, 0xFF, &mut [])
                .await?;
            if cmd16_r1 != 0x00 {
                return Err(SdProbeError::Cmd16Unexpected(cmd16_r1));
            }
        }

        self.apply_data_clock().await?;

        let mut ocr = [0u8; 4];
        let cmd58_r1 = self.send_command(SD_CMD58, 0, 0xFD, &mut ocr).await?;
        if cmd58_r1 != 0x00 {
            return Err(SdProbeError::Cmd58Unexpected(cmd58_r1));
        }

        let cmd9_r1 = self.send_command_hold_cs(SD_CMD9, 0, 0xAF, &mut []).await?;
        if cmd9_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd9Unexpected(cmd9_r1));
        }
        let csd = self.read_data_block().await?;
        self.end_transaction().await;
        let capacity_bytes =
            decode_capacity_bytes(&csd).ok_or(SdProbeError::CapacityDecodeFailed)?;
        let high_capacity = (ocr[0] & 0x40) != 0;
        let filesystem = self.detect_filesystem(high_capacity).await?;

        let status = SdProbeStatus {
            version: card_version,
            high_capacity,
            capacity_bytes,
            filesystem,
        };
        self.high_capacity = Some(high_capacity);
        Ok(status)
    }

    async fn apply_init_clock(&mut self) -> Result<(), SdProbeError> {
        let config = SpiConfig::default()
            .with_mode(SpiMode::_0)
            .with_frequency(Rate::from_khz(SD_INIT_SPI_RATE_KHZ));
        self.spi.apply_config(&config)?;
        Ok(())
    }

    async fn apply_data_clock(&mut self) -> Result<(), SdProbeError> {
        let config = SpiConfig::default()
            .with_mode(SpiMode::_0)
            .with_frequency(Rate::from_mhz(SD_DATA_SPI_RATE_MHZ));
        self.spi.apply_config(&config)?;
        Ok(())
    }

    async fn retry_delay(&self) {
        Timer::after_millis(1).await;
    }

    async fn detect_filesystem(
        &mut self,
        high_capacity: bool,
    ) -> Result<SdFilesystem, SdProbeError> {
        let mut sector = [0u8; SD_SECTOR_SIZE];
        self.read_data_sector_512_into(0, high_capacity, &mut sector)
            .await?;
        if let Some(fs) = detect_vbr_filesystem(&sector) {
            return Ok(fs);
        }

        let mut partition_type = 0u8;
        let mut partition_lba = 0u32;
        for idx in 0..4usize {
            let off = 446 + idx * 16;
            let p_type = sector[off + 4];
            let start = u32::from_le_bytes([
                sector[off + 8],
                sector[off + 9],
                sector[off + 10],
                sector[off + 11],
            ]);
            if p_type != 0 && start != 0 {
                partition_type = p_type;
                partition_lba = start;
                break;
            }
        }

        if partition_lba == 0 {
            return Ok(SdFilesystem::Unknown);
        }

        if partition_type == 0xEE {
            // Protective MBR (GPT). Read the first GPT partition entry.
            self.read_data_sector_512_into(2, high_capacity, &mut sector)
                .await?;
            let start = u64::from_le_bytes([
                sector[32],
                sector[33],
                sector[34],
                sector[35],
                sector[36],
                sector[37],
                sector[38],
                sector[39],
            ]);
            if start != 0 && start <= u32::MAX as u64 {
                partition_lba = start as u32;
            }
        }

        self.read_data_sector_512_into(partition_lba, high_capacity, &mut sector)
            .await?;
        Ok(detect_vbr_filesystem(&sector).unwrap_or(SdFilesystem::Unknown))
    }
}
