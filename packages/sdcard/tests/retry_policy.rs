#![cfg(feature = "host-tests")]

use sdcard::fat::{FatEngineError, SdFatError};

#[test]
fn transport_failure_retry_classification_excludes_logical_fat_errors() {
    assert!(FatEngineError::TimedOut.is_transport_failure());
    assert!(!FatEngineError::Fat(SdFatError::AlreadyExists).is_transport_failure());
    assert!(!FatEngineError::Fat(SdFatError::NotEmpty).is_transport_failure());
}
