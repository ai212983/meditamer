//! Mirrored product tree for the Abort fence harness.
//!
//! Leaf modules pull the real production sources via `#[path]`; siblings
//! are host stand-ins only at the hardware boundary (power rail, FAT
//! transport, probe liveness, asset-path constants, observability filter).
//! `#[path]` below is relative to this directory.

#[allow(dead_code)]
pub mod types {
    pub const SD_PATH_MAX: usize = sdcard::SD_PATH_MAX;

    /// Probe liveness stand-in. Only `is_initialized` exists because that
    /// is the only probe method the pulled upload code calls.
    #[derive(Debug, Default)]
    pub struct FakeProbe {
        pub initialized: bool,
    }

    impl FakeProbe {
        pub fn new(initialized: bool) -> Self {
            Self { initialized }
        }

        pub fn is_initialized(&self) -> bool {
            self.initialized
        }
    }

    pub type SdProbeDriver = FakeProbe;

    include!(concat!(env!("OUT_DIR"), "/upload_sd_types.rs"));
}

#[path = "storage.rs"]
pub mod storage;
