#![cfg(feature = "host-tests")]

use sdcard::fat::{FatEngineError, SdFatError};

const PROBE_BUS_SOURCE: &str = include_str!("../src/probe/mod.rs");

#[test]
fn transport_failure_retry_classification_excludes_logical_fat_errors() {
    assert!(FatEngineError::TimedOut.is_transport_failure());
    assert!(!FatEngineError::Fat(SdFatError::AlreadyExists).is_transport_failure());
    assert!(!FatEngineError::Fat(SdFatError::NotEmpty).is_transport_failure());
}

#[test]
fn spi_dma_transfer_is_awaited_to_completion_instead_of_raced_by_a_timer() {
    assert!(PROBE_BUS_SOURCE.contains("SpiBus::transfer_in_place(self, words)"));
    assert!(!PROBE_BUS_SOURCE.contains("with_timeout"));
    assert!(!PROBE_BUS_SOURCE.contains("DmaTransferTimeout"));
}
