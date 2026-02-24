use esp_hal::{
    gpio::Output,
    spi::{master::Spi, Error as SpiError},
    Blocking,
};

const SD_CMD0: u8 = 0;
const SD_CMD8: u8 = 8;
const SD_CMD9: u8 = 9;
const SD_CMD17: u8 = 17;
const SD_CMD55: u8 = 55;
const SD_ACMD41: u8 = 41;
const SD_CMD58: u8 = 58;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdCardVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdProbeStatus {
    pub version: SdCardVersion,
    pub high_capacity: bool,
    pub capacity_bytes: u64,
    pub filesystem: SdFilesystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdFilesystem {
    ExFat,
    Fat32,
    Fat16,
    Fat12,
    Ntfs,
    Unknown,
}

#[derive(Debug)]
pub enum SdProbeError {
    Spi(SpiError),
    Cmd0Failed(u8),
    Cmd8Unexpected(u8),
    Cmd8EchoMismatch([u8; 4]),
    Acmd41Timeout(u8),
    Cmd58Unexpected(u8),
    Cmd9Unexpected(u8),
    Cmd17Unexpected(u8),
    NoResponse(u8),
    DataTokenTimeout(u8),
    DataTokenUnexpected(u8, u8),
    CapacityDecodeFailed,
}

impl From<SpiError> for SdProbeError {
    fn from(value: SpiError) -> Self {
        Self::Spi(value)
    }
}

pub struct SdCardProbe<'d> {
    spi: Spi<'d, Blocking>,
    cs: Output<'d>,
}

impl<'d> SdCardProbe<'d> {
    pub fn new(spi: Spi<'d, Blocking>, mut cs: Output<'d>) -> Self {
        cs.set_high();
        Self { spi, cs }
    }

    pub fn probe(&mut self) -> Result<SdProbeStatus, SdProbeError> {
        self.cs.set_high();
        self.send_dummy_clocks(10)?;

        let mut cmd0_r1 = 0xFFu8;
        for _ in 0..16 {
            cmd0_r1 = self.send_command(SD_CMD0, 0, 0x95, &mut [])?;
            if cmd0_r1 == 0x01 {
                break;
            }
        }
        if cmd0_r1 != 0x01 {
            return Err(SdProbeError::Cmd0Failed(cmd0_r1));
        }

        let mut r7 = [0u8; 4];
        let cmd8_r1 = self.send_command(SD_CMD8, 0x0000_01AA, 0x87, &mut r7)?;
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
            let _ = self.send_command(SD_CMD55, 0, 0x65, &mut [])?;
            acmd41_r1 = self.send_command(SD_ACMD41, acmd41_arg, 0x77, &mut [])?;
            if acmd41_r1 == 0x00 {
                acmd41_ok = true;
                break;
            }
            self.tiny_spin_delay();
        }
        if !acmd41_ok {
            return Err(SdProbeError::Acmd41Timeout(acmd41_r1));
        }

        let mut ocr = [0u8; 4];
        let cmd58_r1 = self.send_command(SD_CMD58, 0, 0xFD, &mut ocr)?;
        if cmd58_r1 != 0x00 {
            return Err(SdProbeError::Cmd58Unexpected(cmd58_r1));
        }

        let cmd9_r1 = self.send_command_hold_cs(SD_CMD9, 0, 0xAF, &mut [])?;
        if cmd9_r1 != 0x00 {
            self.end_transaction();
            return Err(SdProbeError::Cmd9Unexpected(cmd9_r1));
        }
        let csd = self.read_data_block()?;
        self.end_transaction();
        let capacity_bytes =
            decode_capacity_bytes(&csd).ok_or(SdProbeError::CapacityDecodeFailed)?;
        let filesystem = self.detect_filesystem((ocr[0] & 0x40) != 0)?;

        Ok(SdProbeStatus {
            version: card_version,
            high_capacity: (ocr[0] & 0x40) != 0,
            capacity_bytes,
            filesystem,
        })
    }

    fn send_command(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, true)
    }

    fn send_command_hold_cs(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
    ) -> Result<u8, SdProbeError> {
        self.send_command_inner(cmd, arg, crc, extra_response, false)
    }

    fn send_command_inner(
        &mut self,
        cmd: u8,
        arg: u32,
        crc: u8,
        extra_response: &mut [u8],
        release_cs_after: bool,
    ) -> Result<u8, SdProbeError> {
        let frame = [
            0x40 | cmd,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            crc,
        ];

        self.cs.set_low();
        for byte in frame {
            let _ = self.transfer_byte(byte)?;
        }

        let mut r1 = 0xFFu8;
        let mut got_response = false;
        for _ in 0..16 {
            r1 = self.transfer_byte(0xFF)?;
            if (r1 & 0x80) == 0 {
                got_response = true;
                break;
            }
        }

        if !got_response {
            self.end_transaction();
            return Err(SdProbeError::NoResponse(cmd));
        }

        for slot in extra_response.iter_mut() {
            *slot = self.transfer_byte(0xFF)?;
        }

        if release_cs_after {
            self.end_transaction();
        }
        Ok(r1)
    }

    fn send_dummy_clocks(&mut self, bytes: usize) -> Result<(), SdProbeError> {
        for _ in 0..bytes {
            let _ = self.transfer_byte(0xFF)?;
        }
        Ok(())
    }

    fn transfer_byte(&mut self, byte: u8) -> Result<u8, SdProbeError> {
        let mut frame = [byte];
        self.spi.transfer(&mut frame)?;
        Ok(frame[0])
    }

    fn read_data_block(&mut self) -> Result<[u8; 16], SdProbeError> {
        let mut token = 0xFFu8;
        let mut got_token = false;
        for _ in 0..50_000 {
            token = self.transfer_byte(0xFF)?;
            if token != 0xFF {
                got_token = true;
                break;
            }
        }
        if !got_token {
            return Err(SdProbeError::DataTokenTimeout(SD_CMD9));
        }
        if token != 0xFE {
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD9, token));
        }

        let mut block = [0u8; 16];
        for slot in block.iter_mut() {
            *slot = self.transfer_byte(0xFF)?;
        }
        // Read and discard CRC16.
        let _ = self.transfer_byte(0xFF)?;
        let _ = self.transfer_byte(0xFF)?;
        Ok(block)
    }

    fn read_data_sector_512(
        &mut self,
        lba: u32,
        high_capacity: bool,
    ) -> Result<[u8; 512], SdProbeError> {
        let arg = if high_capacity {
            lba
        } else {
            lba.saturating_mul(512)
        };
        let cmd17_r1 = self.send_command_hold_cs(SD_CMD17, arg, 0xFF, &mut [])?;
        if cmd17_r1 != 0x00 {
            self.end_transaction();
            return Err(SdProbeError::Cmd17Unexpected(cmd17_r1));
        }

        let mut token = 0xFFu8;
        let mut got_token = false;
        for _ in 0..50_000 {
            token = self.transfer_byte(0xFF)?;
            if token != 0xFF {
                got_token = true;
                break;
            }
        }
        if !got_token {
            self.end_transaction();
            return Err(SdProbeError::DataTokenTimeout(SD_CMD17));
        }
        if token != 0xFE {
            self.end_transaction();
            return Err(SdProbeError::DataTokenUnexpected(SD_CMD17, token));
        }

        let mut block = [0u8; 512];
        for slot in block.iter_mut() {
            *slot = self.transfer_byte(0xFF)?;
        }
        // Discard data CRC16.
        let _ = self.transfer_byte(0xFF)?;
        let _ = self.transfer_byte(0xFF)?;
        self.end_transaction();
        Ok(block)
    }

    fn tiny_spin_delay(&self) {
        for _ in 0..5_000 {
            core::hint::spin_loop();
        }
    }

    fn end_transaction(&mut self) {
        self.cs.set_high();
        let _ = self.transfer_byte(0xFF);
    }

    fn detect_filesystem(&mut self, high_capacity: bool) -> Result<SdFilesystem, SdProbeError> {
        let sector0 = self.read_data_sector_512(0, high_capacity)?;
        if let Some(fs) = detect_vbr_filesystem(&sector0) {
            return Ok(fs);
        }

        let mut partition_type = 0u8;
        let mut partition_lba = 0u32;
        for idx in 0..4usize {
            let off = 446 + idx * 16;
            let p_type = sector0[off + 4];
            let start = u32::from_le_bytes([
                sector0[off + 8],
                sector0[off + 9],
                sector0[off + 10],
                sector0[off + 11],
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
            let gpt_entry_sector = self.read_data_sector_512(2, high_capacity)?;
            let start = u64::from_le_bytes([
                gpt_entry_sector[32],
                gpt_entry_sector[33],
                gpt_entry_sector[34],
                gpt_entry_sector[35],
                gpt_entry_sector[36],
                gpt_entry_sector[37],
                gpt_entry_sector[38],
                gpt_entry_sector[39],
            ]);
            if start != 0 && start <= u32::MAX as u64 {
                partition_lba = start as u32;
            }
        }

        let vbr = self.read_data_sector_512(partition_lba, high_capacity)?;
        Ok(detect_vbr_filesystem(&vbr).unwrap_or(SdFilesystem::Unknown))
    }
}

fn decode_capacity_bytes(csd: &[u8; 16]) -> Option<u64> {
    let csd_structure = csd_get_bits(csd, 127, 126) as u8;
    match csd_structure {
        0 => {
            // CSD v1.0 (SDSC)
            let c_size = csd_get_bits(csd, 73, 62) as u64;
            let c_size_mult = csd_get_bits(csd, 49, 47) as u64;
            let read_bl_len = csd_get_bits(csd, 83, 80) as u64;

            let block_len = 1u64.checked_shl(read_bl_len as u32)?;
            let mult = 1u64.checked_shl((c_size_mult + 2) as u32)?;
            let blocknr = (c_size + 1).checked_mul(mult)?;
            blocknr.checked_mul(block_len)
        }
        1 => {
            // CSD v2.0 (SDHC/SDXC)
            let c_size = csd_get_bits(csd, 69, 48) as u64;
            (c_size + 1).checked_mul(512 * 1024)
        }
        _ => None,
    }
}

fn csd_get_bits(csd: &[u8; 16], msb: u8, lsb: u8) -> u32 {
    let mut value = 0u32;
    for bit in (lsb..=msb).rev() {
        let byte_idx = (127 - bit) / 8;
        let bit_in_byte = bit % 8;
        let b = (csd[byte_idx as usize] >> bit_in_byte) & 1;
        value = (value << 1) | (b as u32);
    }
    value
}

fn detect_vbr_filesystem(sector: &[u8; 512]) -> Option<SdFilesystem> {
    if &sector[3..11] == b"EXFAT   " {
        return Some(SdFilesystem::ExFat);
    }
    if &sector[3..11] == b"NTFS    " {
        return Some(SdFilesystem::Ntfs);
    }
    if &sector[82..90] == b"FAT32   " {
        return Some(SdFilesystem::Fat32);
    }
    if &sector[54..62] == b"FAT16   " {
        return Some(SdFilesystem::Fat16);
    }
    if &sector[54..62] == b"FAT12   " {
        return Some(SdFilesystem::Fat12);
    }
    None
}
