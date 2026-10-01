//! Chip-neutral block transport boundary for the FAT engine.

use core::fmt;

/// The fixed error vocabulary exposed by a storage transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportErrorKind {
    Timeout,
    Integrity,
    Unavailable,
    Protocol,
    Bounds,
    Io,
}

/// A compact, driver-neutral transport error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub kind: TransportErrorKind,
    pub driver_detail: &'static str,
    pub diagnostic: Option<u32>,
    pub timeout_wait: Option<TimeoutWait>,
}

/// Progress made while waiting for an SD protocol response before timing out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeoutWait {
    pub elapsed_ms: u32,
    pub active_ms: Option<u32>,
    pub polls: u32,
}

impl TransportError {
    pub const fn new(
        kind: TransportErrorKind,
        driver_detail: &'static str,
        diagnostic: Option<u32>,
    ) -> Self {
        Self {
            kind,
            driver_detail,
            diagnostic,
            timeout_wait: None,
        }
    }

    pub const fn with_timeout_wait(mut self, elapsed_ms: u32, polls: u32) -> Self {
        self.timeout_wait = Some(TimeoutWait {
            elapsed_ms,
            active_ms: None,
            polls,
        });
        self
    }

    pub const fn with_data_token_wait(
        mut self,
        elapsed_ms: u32,
        active_ms: u32,
        polls: u32,
    ) -> Self {
        self.timeout_wait = Some(TimeoutWait {
            elapsed_ms,
            active_ms: Some(active_ms),
            polls,
        });
        self
    }

    pub const fn is_timeout(self) -> bool {
        matches!(self.kind, TransportErrorKind::Timeout)
    }

    pub const fn requires_bus_recovery(self) -> bool {
        self.is_timeout()
            || matches!(
                self.kind,
                TransportErrorKind::Io | TransportErrorKind::Integrity
            )
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.driver_detail, self.kind)
    }
}

/// A 512-byte sector transport. The engine emits contiguous writes as one call
/// so adapters can retain hardware batching (including Inkplate's CMD25 path).
/// Adapters with smaller staging buffers may split it within their I/O deadline.
#[allow(async_fn_in_trait)]
pub trait SectorTransport {
    type Error: Into<TransportError>;

    async fn read_sector(&mut self, lba: u32, sector: &mut [u8; 512]) -> Result<(), Self::Error>;

    async fn write_sector(&mut self, lba: u32, sector: &[u8; 512]) -> Result<(), Self::Error>;

    async fn write_sectors_contiguous(
        &mut self,
        start_lba: u32,
        sectors: &[u8],
    ) -> Result<(), Self::Error>;

    async fn recover_after_timeout(&mut self) {}
}
