//! Shared types for artifact inventory and pruning.
//!
//! See `docs/archive/host-tooling/log-and-artifact-pruning.md` for the historical policy these types
//! implement: recognized flash-payload roles, retention scopes, and the
//! outcome classification used by inventory reporting.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Deserialize;

/// One of the four canonical flash-capture payload roles. Any other binary
/// under `logs/` (diagnostic dumps, fixtures, legacy collections) is not a
/// recognized payload, regardless of extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadRole {
    FirmwareElf,
    AppBin,
    BootloaderBin,
    PartitionTable,
}

impl PayloadRole {
    pub const ALL: [PayloadRole; 4] = [
        PayloadRole::FirmwareElf,
        PayloadRole::AppBin,
        PayloadRole::BootloaderBin,
        PayloadRole::PartitionTable,
    ];

    /// The exact filename this role is recognized by inside a flash-capture
    /// layout (matches `workflows::flash_capture::paths::prepare_output_paths`).
    pub fn file_name(self) -> &'static str {
        match self {
            PayloadRole::FirmwareElf => "firmware.elf",
            PayloadRole::AppBin => "app.bin",
            PayloadRole::BootloaderBin => "bootloader.bin",
            PayloadRole::PartitionTable => "partition-table.bin",
        }
    }

    /// Stable machine-readable label used in report/output rows.
    pub fn label(self) -> &'static str {
        match self {
            PayloadRole::FirmwareElf => "firmware_elf",
            PayloadRole::AppBin => "app_bin",
            PayloadRole::BootloaderBin => "bootloader_bin",
            PayloadRole::PartitionTable => "partition_table",
        }
    }

    /// Whether a retention record with `scope` protects this payload role
    /// from artifact thinning. `evidence` protects the run unit from whole
    /// -unit expiry only (Phase 2), not from payload thinning.
    pub fn protected_by(self, scope: RetentionScope) -> bool {
        match scope {
            RetentionScope::Evidence => false,
            RetentionScope::Reflash => !matches!(self, PayloadRole::FirmwareElf),
            RetentionScope::Debug => true,
        }
    }
}

/// A recognized flash payload discovered inside a run unit.
#[derive(Debug, Clone)]
pub struct Payload {
    pub role: PayloadRole,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified: SystemTime,
}

/// Declared retention scope from a `.retain.json` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetentionScope {
    Evidence,
    Reflash,
    Debug,
}

impl RetentionScope {
    pub fn label(self) -> &'static str {
        match self {
            RetentionScope::Evidence => "evidence",
            RetentionScope::Reflash => "reflash",
            RetentionScope::Debug => "debug",
        }
    }
}

/// A parsed `.retain.json` (run-unit) or `<name>.retain.json` (standalone)
/// retention record. `review_after` is kept as the raw `YYYY-MM-DD` string;
/// callers needing a parsed date use `retention::is_review_due`.
#[derive(Debug, Clone, Deserialize)]
pub struct RetentionRecord {
    pub scope: RetentionScope,
    pub reason: String,
    pub owner: String,
    pub review_after: String,
}

/// Inferred run outcome, used only for inventory reporting in this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    Inconclusive,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::Inconclusive => "inconclusive",
        }
    }
}
