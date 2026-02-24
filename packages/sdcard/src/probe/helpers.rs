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
