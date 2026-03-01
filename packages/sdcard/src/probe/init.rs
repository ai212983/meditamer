impl<'d> SdCardProbe<'d> {
    pub async fn init(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.cached_sector_lba = None;
        self.probe().await
    }

    pub async fn probe(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.apply_init_clock().await?;
        self.cs.set_high();
        self.send_dummy_clocks(10).await?;
        self.enter_idle_state().await?;

        let card_version = self.detect_card_version().await?;
        self.wait_acmd41_ready(card_version).await?;
        self.configure_legacy_block_len(card_version).await?;

        self.apply_data_clock().await?;
        let ocr = self.read_ocr().await?;
        let capacity_bytes = self.read_capacity_bytes().await?;
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

    async fn enter_idle_state(&mut self) -> Result<(), SdProbeError> {
        let mut cmd0_r1 = 0xFFu8;
        for _ in 0..16 {
            cmd0_r1 = self.send_command(SD_CMD0, 0, 0x95, &mut []).await?;
            if cmd0_r1 == 0x01 {
                return Ok(());
            }
        }
        Err(SdProbeError::Cmd0Failed(cmd0_r1))
    }

    async fn detect_card_version(&mut self) -> Result<SdCardVersion, SdProbeError> {
        let mut r7 = [0u8; 4];
        let cmd8_r1 = self
            .send_command(SD_CMD8, 0x0000_01AA, 0x87, &mut r7)
            .await?;

        if cmd8_r1 == 0x01 {
            if r7[2] != 0x01 || r7[3] != 0xAA {
                return Err(SdProbeError::Cmd8EchoMismatch(r7));
            }
            return Ok(SdCardVersion::V2);
        }
        if (cmd8_r1 & 0x04) != 0 {
            return Ok(SdCardVersion::V1);
        }
        Err(SdProbeError::Cmd8Unexpected(cmd8_r1))
    }

    async fn wait_acmd41_ready(&mut self, card_version: SdCardVersion) -> Result<(), SdProbeError> {
        let acmd41_arg = if card_version == SdCardVersion::V2 {
            0x4000_0000
        } else {
            0
        };

        let mut acmd41_r1 = 0xFFu8;
        for _ in 0..200 {
            let _ = self.send_command(SD_CMD55, 0, 0x65, &mut []).await?;
            acmd41_r1 = self
                .send_command(SD_ACMD41, acmd41_arg, 0x77, &mut [])
                .await?;
            if acmd41_r1 == 0x00 {
                return Ok(());
            }
            self.retry_delay().await;
        }
        Err(SdProbeError::Acmd41Timeout(acmd41_r1))
    }

    async fn configure_legacy_block_len(
        &mut self,
        card_version: SdCardVersion,
    ) -> Result<(), SdProbeError> {
        if card_version != SdCardVersion::V1 {
            return Ok(());
        }

        let cmd16_r1 = self
            .send_command(SD_CMD16, SD_SECTOR_SIZE as u32, 0xFF, &mut [])
            .await?;
        if cmd16_r1 != 0x00 {
            return Err(SdProbeError::Cmd16Unexpected(cmd16_r1));
        }
        Ok(())
    }

    async fn read_ocr(&mut self) -> Result<[u8; 4], SdProbeError> {
        let mut ocr = [0u8; 4];
        let cmd58_r1 = self.send_command(SD_CMD58, 0, 0xFD, &mut ocr).await?;
        if cmd58_r1 != 0x00 {
            return Err(SdProbeError::Cmd58Unexpected(cmd58_r1));
        }
        Ok(ocr)
    }

    async fn read_capacity_bytes(&mut self) -> Result<u64, SdProbeError> {
        let cmd9_r1 = self.send_command_hold_cs(SD_CMD9, 0, 0xAF, &mut []).await?;
        if cmd9_r1 != 0x00 {
            self.end_transaction().await;
            return Err(SdProbeError::Cmd9Unexpected(cmd9_r1));
        }
        let csd = self.read_data_block().await?;
        self.end_transaction().await;
        decode_capacity_bytes(&csd).ok_or(SdProbeError::CapacityDecodeFailed)
    }

    async fn apply_init_clock(&mut self) -> Result<(), SdProbeError> {
        let config = SpiConfig::default()
            .with_mode(SpiMode::_0)
            .with_frequency(Rate::from_khz(SD_INIT_SPI_RATE_KHZ));
        self.spi.apply_config(&config)?;
        Ok(())
    }

    async fn apply_data_clock(&mut self) -> Result<(), SdProbeError> {
        // Keep this tunable via build-time env so throughput experiments can
        // sweep SD SPI data clock safely without code churn.
        let data_rate_mhz = Self::data_spi_rate_mhz();
        esp_println::println!("sdprobe: data_spi_mhz={}", data_rate_mhz);
        let config = SpiConfig::default()
            .with_mode(SpiMode::_0)
            .with_frequency(Rate::from_mhz(data_rate_mhz));
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
