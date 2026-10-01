//! Complete USB flash for the single-production layout (ADR-0014 Phase 2):
//! bootloader, partition table, factory (updater) image, production
//! (`ota_0`) image, and the initial `otadata` this module's `otadata`
//! sibling constructs — all in one `esptool.py write_flash` invocation,
//! mirroring `tools/hostctl/src/workflows/flash_capture/flash.rs::build_full_flash_command`'s
//! A/B equivalent but with a fifth region (`factory`) and a real initial
//! `otadata` image instead of ESP-IDF's blank default.

use std::{path::Path, process::Command};

use anyhow::{bail, Context, Result};

use crate::idf_env::IdfEnv;

use super::otadata::build_initial_otadata;

pub struct CompleteFlashInputs<'a> {
    pub idf_env: &'a IdfEnv,
    pub port: &'a str,
    pub flash_baud: u32,
    pub bootloader_bin: &'a Path,
    pub partition_table_bin: &'a Path,
    pub factory_bin: &'a Path,
    pub production_bin: &'a Path,
    /// Where to write the constructed initial `otadata` image so it can be
    /// handed to `esptool.py` as a file argument like every other region.
    pub otadata_scratch_path: &'a Path,
}

/// Fixed by `config/partitions-single-production.csv` (ADR-0014); not read
/// back from the partition table here because the write offsets below are
/// exactly what a mismatch would need to be caught against in the first
/// place — if the CSV ever changes these must change together, deliberately,
/// not silently follow along.
const BOOTLOADER_OFFSET: u32 = 0x1000;
const PARTITION_TABLE_OFFSET: u32 = 0x8000;
const OTADATA_OFFSET: u32 = 0xf000;
const FACTORY_OFFSET: u32 = 0x20000;
const OTA_0_OFFSET: u32 = 0x80000;

/// Writes the complete single-production flash and returns nothing on
/// success — `esptool.py write_flash` already verifies checksums as it
/// writes; a non-zero exit is the failure signal.
pub fn run_complete_flash(inputs: CompleteFlashInputs<'_>) -> Result<()> {
    for (label, path) in [
        ("bootloader", inputs.bootloader_bin),
        ("partition table", inputs.partition_table_bin),
        ("factory image", inputs.factory_bin),
        ("production image", inputs.production_bin),
    ] {
        if !path.is_file() {
            bail!("{label} not found at {}", path.display());
        }
    }

    let partition_table_bin = std::fs::read(inputs.partition_table_bin)
        .with_context(|| format!("failed to read {}", inputs.partition_table_bin.display()))?;
    let otadata = build_initial_otadata(&partition_table_bin)
        .context("failed to construct the initial otadata image")?;
    std::fs::write(inputs.otadata_scratch_path, &otadata).with_context(|| {
        format!(
            "failed to write otadata scratch file {}",
            inputs.otadata_scratch_path.display()
        )
    })?;

    let mut command = Command::new(&inputs.idf_env.python_bin);
    command
        .arg(&inputs.idf_env.esptool_bin)
        .args(["--chip", "esp32", "--port", inputs.port, "--baud"])
        .arg(inputs.flash_baud.to_string())
        .args([
            "write_flash",
            "-z",
            "--flash_mode",
            "dio",
            "--flash_freq",
            "40m",
            "--flash_size",
            "4MB",
        ])
        .arg(format!("{BOOTLOADER_OFFSET:#x}"))
        .arg(inputs.bootloader_bin)
        .arg(format!("{PARTITION_TABLE_OFFSET:#x}"))
        .arg(inputs.partition_table_bin)
        .arg(format!("{OTADATA_OFFSET:#x}"))
        .arg(inputs.otadata_scratch_path)
        .arg(format!("{FACTORY_OFFSET:#x}"))
        .arg(inputs.factory_bin)
        .arg(format!("{OTA_0_OFFSET:#x}"))
        .arg(inputs.production_bin);

    let status = command
        .status()
        .context("failed to run esptool.py write_flash")?;
    if !status.success() {
        bail!("esptool.py write_flash failed with status {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_input_files_are_reported_before_touching_the_device() {
        let idf_env = IdfEnv {
            idf_root: "/nonexistent".into(),
            python_bin: "/nonexistent/python".into(),
            esptool_bin: "/nonexistent/esptool.py".into(),
            idf_py_bin: None,
        };
        let missing = Path::new("/nonexistent/missing.bin");
        let err = run_complete_flash(CompleteFlashInputs {
            idf_env: &idf_env,
            port: "/dev/null",
            flash_baud: 460_800,
            bootloader_bin: missing,
            partition_table_bin: missing,
            factory_bin: missing,
            production_bin: missing,
            otadata_scratch_path: missing,
        })
        .unwrap_err();
        assert!(err.to_string().contains("bootloader not found"));
    }
}
