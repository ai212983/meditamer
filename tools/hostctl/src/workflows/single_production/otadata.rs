//! Host-side construction of the initial `otadata` image for a complete USB
//! flash of the single-production layout (ADR-0014 Phase 2, invariant 2).
//!
//! Both redundant `otadata` sectors start erased (`0xFF`); this writes
//! exactly one record — `ota_seq = 1`, [`OtaImageState::New`] — to sector 0,
//! leaving sector 1 blank. That is the same state a runtime SD install
//! leaves behind after activation (`docs/plans/single-production-sd-recovery-updater.md`),
//! so a device's very first boot after a complete USB flash goes through the
//! identical candidate-confirmation path as any later install, rather than
//! a distinct "freshly flashed" special case.
//!
//! The record layout and its CRC32 are reproduced from the pinned
//! `esp_ota_select_entry_t` / `bootloader_common_ota_select_crc` in
//! `.embuild/espressif/esp-idf/v5.5.2/components/bootloader_support/{include/esp_flash_partitions.h,src/bootloader_common_loader.c}`
//! — the CRC is `esp_rom_crc32_le(0xFFFFFFFF, &ota_seq, 4)`. That is
//! *not* plain `CRC_32_ISO_HDLC` (`crc` crate's named preset, `init =
//! 0xFFFFFFFF`) despite matching poly/refin/refout/xorout — see `crc32`
//! below for why, and for how a first attempt at this got it wrong in a way
//! that only a real board caught. `ota_seq = 1` specifically (not the `0` a
//! naive multi-slot rotation would compute for a single-`ota_0` layout)
//! avoids a `seq - 1` unsigned underflow in
//! `esp_bootloader_esp_idf::ota::Ota::current_app_partition`'s
//! sequence-to-slot math — harmless in release (`x % 1 == 0` regardless of
//! wraparound) but a debug-build panic waiting to happen at `seq == 0`.
//!
//! Round-tripped through the same `esp_bootloader_esp_idf::ota::Ota` the
//! firmware itself uses to read `otadata` at boot (see tests below) — but
//! note what that does and doesn't prove: `Ota::current_app_partition`
//! never actually checks the CRC field (only the real bootloader does), so
//! this round-trip alone was not enough to catch a wrong CRC. It takes a
//! real device to fully verify this file — see `crc32`'s doc comment.

use anyhow::{ensure, Context, Result};
use esp_bootloader_esp_idf::partitions::{
    self, AppPartitionSubType, DataPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};

/// Where `espflash`/`esptool` place the partition table (`tools/hostctl/src/workflows/flash_capture/flash.rs`
/// hardcodes the same offset for the same reason: it's a fixed ESP-IDF ABI
/// constant, not something read out of the table itself).
const PARTITION_TABLE_OFFSET: u32 = 0x8000;
/// `esp_ota_select_entry_t` is 32 bytes ("friendly to flash encryption").
const RECORD_LEN: usize = 32;
const RECORD_SEQ_LEN: usize = 4;
const RECORD_STATE_OFFSET: usize = 24;
const RECORD_CRC_OFFSET: usize = 28;
/// The sequence number this workflow always writes. See module docs for why
/// `1`, not `0`.
const INITIAL_SEQUENCE: u32 = 1;

/// A flat byte buffer standing in for "the whole flash" —
/// `esp_bootloader_esp_idf::partitions::FlashRegion` computes absolute
/// addresses (`partition offset + in-partition offset`) and expects its
/// backing storage to answer at those addresses directly, the same
/// assumption a real `esp-storage` `FlashStorage` makes on-device.
struct WholeFlash {
    data: Vec<u8>,
}

impl WholeFlash {
    fn blank(len: usize) -> Self {
        Self {
            data: vec![0xFFu8; len],
        }
    }
}

impl embedded_storage::ReadStorage for WholeFlash {
    type Error = std::convert::Infallible;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        bytes.copy_from_slice(&self.data[start..start + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl embedded_storage::Storage for WholeFlash {
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let start = offset as usize;
        self.data[start..start + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
}

// NOT `crc::CRC_32_ISO_HDLC` (the textbook/zlib "CRC-32", `init=0xFFFFFFFF`)
// despite matching its poly/refin/refout/xorout — this is genuinely a
// different algorithm. `bootloader_common_ota_select_crc` calls
// `esp_rom_crc32_le(0xFFFFFFFF, &ota_seq, 4)`, and that function's body
// (`.embuild/.../esp_rom/{linux,patches}/esp_rom_crc.c`) is
// `crc = ~crc_in; loop; return ~crc;` — passing `crc_in = 0xFFFFFFFF` makes
// `~crc_in == 0`, so the table loop's *entry* register is `0`, not
// `0xFFFFFFFF`. First got this wrong by treating the *parameter* value as
// the loop-entry register directly and cross-checking against
// `zlib.crc32(data)` (implicit `value=0`) instead of `zlib.crc32(data,
// 0xFFFFFFFF)` — the two differ, and only the latter matches hardware
// (confirmed by writing both to a real board's `otadata` and reading back
// which one `bootloader_utility_get_selected_boot_partition` accepted).
// `init: 0` here is exactly that loop-entry register.
const ESP_ROM_CRC32_LE: crc::Algorithm<u32> = crc::Algorithm {
    width: 32,
    poly: 0x04c1_1db7,
    init: 0,
    refin: true,
    refout: true,
    xorout: 0xffff_ffff,
    check: 0,
    residue: 0,
};

fn crc32(bytes: &[u8]) -> u32 {
    crc::Crc::<u32>::new(&ESP_ROM_CRC32_LE).checksum(bytes)
}

/// Encodes one `esp_ota_select_entry_t`: `ota_seq` (LE u32), a `seq_label`
/// left at its erased value (unused by the bootloader; the reference
/// vectors this was checked against — `esp_bootloader_esp_idf`'s own
/// `ota.rs` unit tests — leave it `0xFF` too, never zeroed), `ota_state`
/// (LE u32), then the CRC32 of just the `ota_seq` bytes.
fn encode_record(seq: u32, state: esp_bootloader_esp_idf::ota::OtaImageState) -> [u8; RECORD_LEN] {
    let mut record = [0xFFu8; RECORD_LEN];
    record[..RECORD_SEQ_LEN].copy_from_slice(&seq.to_le_bytes());
    record[RECORD_STATE_OFFSET..RECORD_CRC_OFFSET].copy_from_slice(&(state as u32).to_le_bytes());
    let crc = crc32(&record[..RECORD_SEQ_LEN]);
    record[RECORD_CRC_OFFSET..].copy_from_slice(&crc.to_le_bytes());
    record
}

/// Builds the initial `otadata` partition image (both sectors) for a
/// complete USB flash of `partition_table_bin` (the binary form of
/// `config/partitions-single-production.csv`, e.g. from
/// `espflash partition-table ... --to-binary`). Returns exactly
/// `otadata`'s declared size (8 KiB: two 4 KiB sectors) — sector 0 holding
/// the one `ota_seq=1`/`New` record, sector 1 left erased.
pub fn build_initial_otadata(partition_table_bin: &[u8]) -> Result<Vec<u8>> {
    // Generous enough to hold any partition offset this table declares; the
    // otadata partition itself is what actually gets sliced out and returned.
    let mut flash = WholeFlash::blank(4 * 1024 * 1024);
    let pt_end = PARTITION_TABLE_OFFSET as usize + partition_table_bin.len();
    ensure!(pt_end <= flash.data.len(), "partition table does not fit the scratch flash image");
    flash.data[PARTITION_TABLE_OFFSET as usize..pt_end].copy_from_slice(partition_table_bin);

    let mut table_buf = [0u8; PARTITION_TABLE_MAX_LEN];
    let table = partitions::read_partition_table(&mut flash, &mut table_buf)
        .map_err(|err| anyhow::anyhow!("failed to parse partition table: {err}"))?;
    let ota_entry = table
        .find_partition(PartitionType::Data(DataPartitionSubType::Ota))
        .map_err(|err| anyhow::anyhow!("{err}"))?
        .context("partition table has no otadata partition")?;
    let otadata_offset = ota_entry.offset() as usize;
    let otadata_len = ota_entry.len() as usize;
    ensure!(
        otadata_len == 0x2000,
        "unexpected otadata size {otadata_len:#x}, expected 0x2000"
    );

    let mut region = ota_entry.as_embedded_storage(&mut flash);
    let sector0 = encode_record(INITIAL_SEQUENCE, esp_bootloader_esp_idf::ota::OtaImageState::New);
    // `Ota` has no public "write this exact record" method — by design, its
    // API models the multi-slot A/B rotation, not a direct write — so write
    // sector 0 (offset 0x0; sector 1 is 0x1000, `esp_ota_select_entry_t` is
    // 32 bytes) through the same `embedded_storage::Storage` trait `Ota`
    // itself is generic over, before handing the region to `Ota::new`.
    embedded_storage::Storage::write(&mut region, 0x0000, &sector0)
        .map_err(|err| anyhow::anyhow!("{err}"))?;

    let mut ota =
        esp_bootloader_esp_idf::ota::Ota::new(region, 1).map_err(|err| anyhow::anyhow!("{err}"))?;

    // Round-trip through the exact same reader the firmware uses at boot —
    // if this doesn't come back New/Ota0, something about the record layout
    // is wrong and we want to fail here, not on hardware.
    let selected = ota.current_app_partition().map_err(|err| anyhow::anyhow!("{err}"))?;
    ensure!(
        selected == AppPartitionSubType::Ota0,
        "constructed otadata resolves to {selected:?}, expected Ota0"
    );
    let state = ota.current_ota_state().map_err(|err| anyhow::anyhow!("{err}"))?;
    ensure!(
        state == esp_bootloader_esp_idf::ota::OtaImageState::New,
        "constructed otadata resolves to state {state:?}, expected New"
    );

    Ok(flash.data[otadata_offset..otadata_offset + otadata_len].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_production_partition_table() -> Vec<u8> {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("resolve repo root");
        let csv = repo_root.join("config/partitions-single-production.csv");
        let out = tempfile::NamedTempFile::new().expect("temp file");
        let status = std::process::Command::new("espflash")
            .args(["partition-table", "--skip-update-check"])
            .arg(&csv)
            .args(["--to-binary", "-o"])
            .arg(out.path())
            .status()
            .expect("run espflash partition-table");
        assert!(status.success(), "espflash partition-table failed");
        std::fs::read(out.path()).expect("read converted partition table")
    }

    // These match `esp-bootloader-esp-idf`'s own `ota.rs` unit-test fixtures
    // (`SLOT_COUNT_1_VALID`/`SLOT_COUNT_2_NEW`) — an earlier version of this
    // file dismissed those as unvalidated filler and asserted a *different*,
    // wrong CRC here instead (`crc::CRC_32_ISO_HDLC`, i.e. `init=0xFFFFFFFF`).
    // That value passed every check available without hardware — it matched
    // an independent `zlib.crc32(data)` call, and `Ota::current_app_partition`
    // happily accepted the record it produced — and was still wrong: a real
    // board rejected it (`bootloader: ota data partition invalid, falling
    // back to factory`) because `Ota`'s reader never actually checks the CRC
    // field, only the real bootloader does.
    //
    // The bug was `zlib.crc32(data)` versus `zlib.crc32(data, 0xFFFFFFFF)` —
    // not the same computation. `bootloader_common_ota_select_crc` calls
    // `esp_rom_crc32_le(0xFFFFFFFF, &ota_seq, 4)`
    // (`.embuild/espressif/esp-idf/v5.5.2/components/bootloader_support/src/bootloader_common_loader.c`),
    // whose body (`esp_rom/{linux,patches}/esp_rom_crc.c`) is
    // `crc = ~crc_in; loop table-driven update; return ~crc;`. With
    // `crc_in = 0xFFFFFFFF`, `~crc_in == 0`, so the table loop's *entry*
    // register is `0` — not `0xFFFFFFFF`, the standard CRC-32/ISO-HDLC init
    // that `zlib.crc32(data)` (implicit `value=0`) reproduces.
    // `zlib.crc32(data, 0xFFFFFFFF)` reproduces the *correct* one, confirmed
    // against a real board: written to `otadata` and read back byte-for-byte
    // (`espflash read-flash`), then observed to make
    // `bootloader_utility_get_selected_boot_partition` accept the record and
    // boot `ota_0` instead of falling back to `factory`.
    #[test]
    fn crc_matches_the_esp_idf_rom_algorithm_confirmed_on_real_hardware() {
        let record = encode_record(1, esp_bootloader_esp_idf::ota::OtaImageState::New);
        assert_eq!(&record[28..32], &[154, 152, 67, 71]); // zlib.crc32([1,0,0,0], 0xFFFFFFFF)
        assert_eq!(&record[24..28], &[0, 0, 0, 0]); // New
        assert_eq!(&record[..4], &[1, 0, 0, 0]);
        assert_eq!(&record[4..24], &[0xFFu8; 20]);
    }

    #[test]
    fn crc_varies_correctly_with_the_sequence_number() {
        let record = encode_record(2, esp_bootloader_esp_idf::ota::OtaImageState::New);
        assert_eq!(&record[28..32], &[116, 55, 246, 85]); // zlib.crc32([2,0,0,0], 0xFFFFFFFF)
    }

    #[test]
    fn builds_an_initial_otadata_image_the_real_ota_reader_accepts() {
        let partition_table = single_production_partition_table();
        let otadata = build_initial_otadata(&partition_table).expect("build otadata");
        assert_eq!(otadata.len(), 0x2000);
        assert_eq!(&otadata[..4], &1u32.to_le_bytes(), "sector 0 ota_seq");
        assert_eq!(&otadata[24..28], &0u32.to_le_bytes(), "sector 0 ota_state == New");
        assert!(
            otadata[0x1000..].iter().all(|byte| *byte == 0xFF),
            "sector 1 must stay erased — only one record is ever written"
        );
    }

    #[test]
    fn rejects_a_partition_table_without_an_otadata_partition() {
        let no_otadata = single_production_partition_table();
        // Corrupt just enough that read_partition_table still parses the
        // frame but otadata_esque metadata can't be found — cheapest way to
        // exercise the error path without hand-building a whole second CSV.
        let mut broken = no_otadata;
        for byte in broken.iter_mut() {
            *byte = 0xFF;
        }
        assert!(build_initial_otadata(&broken).is_err());
    }
}
