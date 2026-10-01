//! SPI adapter for the shared sector boundary. SPI detail stays here.
use crate::probe::{SdCardProbe, SdProbeError, SdSpiBus};
use crate::transport::{SectorTransport, TransportError, TransportErrorKind as Kind};

impl From<SdProbeError> for TransportError {
    fn from(error: SdProbeError) -> Self {
        let timeout_wait = match &error {
            SdProbeError::DataTokenTimeout {
                elapsed_ms,
                active_ms,
                polls,
                ..
            } => Some((*elapsed_ms, *active_ms, *polls)),
            _ => None,
        };
        let kind = if error.is_timeout() {
            Kind::Timeout
        } else {
            match &error {
                SdProbeError::ReadCrcMismatch { .. } => Kind::Integrity,
                SdProbeError::Spi(_) => Kind::Io,
                SdProbeError::NotInitialized => Kind::Unavailable,
                SdProbeError::WriteLengthInvalid(_) => Kind::Bounds,
                _ => Kind::Protocol,
            }
        };
        let (detail, diagnostic) = probe_error_details(error);
        let transport_error = Self::new(kind, detail, diagnostic);
        match timeout_wait {
            Some((elapsed_ms, active_ms, polls)) => {
                transport_error.with_data_token_wait(elapsed_ms, active_ms, polls)
            }
            None => transport_error,
        }
    }
}

fn probe_error_details(error: SdProbeError) -> (&'static str, Option<u32>) {
    match error {
        SdProbeError::Spi(e) => (spi_detail(e), None),
        SdProbeError::SpiConfig(_) => ("spi configuration", None),
        SdProbeError::Cmd0Failed(v) => ("CMD0 response", Some(v.into())),
        SdProbeError::Cmd8Unexpected(v) => ("CMD8 response", Some(v.into())),
        SdProbeError::Cmd8EchoMismatch(v) => ("CMD8 echo", Some(u32::from_be_bytes(v))),
        SdProbeError::Acmd41Timeout(v) => ("ACMD41 timeout", Some(v.into())),
        SdProbeError::Cmd58Unexpected(v) => ("CMD58 response", Some(v.into())),
        SdProbeError::Cmd9Unexpected(v) => ("CMD9 response", Some(v.into())),
        SdProbeError::Cmd16Unexpected(v) => ("CMD16 response", Some(v.into())),
        SdProbeError::Cmd17Unexpected(v) => ("CMD17 response", Some(v.into())),
        SdProbeError::Cmd24Unexpected(v) => ("CMD24 response", Some(v.into())),
        SdProbeError::Cmd25Unexpected(v) => ("CMD25 response", Some(v.into())),
        SdProbeError::Cmd13Unexpected(a, b) => {
            ("CMD13 response", Some((u32::from(a) << 8) | u32::from(b)))
        }
        SdProbeError::NoResponse(v) => ("no command response", Some(v.into())),
        SdProbeError::DataTokenTimeout { command, .. } => {
            ("data token timeout", Some(command.into()))
        }
        SdProbeError::DataTokenUnexpected(a, b) => {
            ("data token", Some((u32::from(a) << 8) | u32::from(b)))
        }
        SdProbeError::ReadCrcMismatch {
            command: cmd,
            expected,
            received,
        } => (
            if cmd == 9 {
                "CMD9 read CRC"
            } else {
                "CMD17 read CRC"
            },
            Some((u32::from(expected) << 16) | u32::from(received)),
        ),
        SdProbeError::WriteDataRejected(v) => ("write data rejected", Some(v.into())),
        SdProbeError::InitDeadlineExceeded => ("initialization deadline", None),
        SdProbeError::WriteBusyTimeout { elapsed_ms, .. } => {
            ("write busy timeout", Some(elapsed_ms))
        }
        SdProbeError::WriteLengthInvalid(v) => ("invalid write length", Some(v as u32)),
        SdProbeError::NotInitialized => ("not initialized", None),
        SdProbeError::CapacityDecodeFailed => ("capacity decode", None),
    }
}

fn spi_detail(error: esp_hal::spi::Error) -> &'static str {
    use esp_hal::{dma::DmaError, spi::Error};
    match error {
        Error::DmaError(e) => match e {
            DmaError::InvalidAlignment(_) => "SPI DMA alignment",
            DmaError::OutOfDescriptors => "SPI DMA descriptors exhausted",
            DmaError::DescriptorError => "SPI DMA descriptor error",
            DmaError::Overflow => "SPI DMA overflow",
            DmaError::BufferTooSmall => "SPI DMA buffer too small",
            DmaError::UnsupportedMemoryRegion => "SPI DMA memory region",
            DmaError::InvalidChunkSize => "SPI DMA chunk size",
        },
        Error::MaxDmaTransferSizeExceeded => "SPI DMA maximum transfer size",
        Error::FifoSizeExeeded => "SPI FIFO size",
        Error::Unsupported => "SPI unsupported operation",
        _ => "SPI unknown",
    }
}

impl<S: SdSpiBus> SectorTransport for SdCardProbe<'_, S> {
    type Error = SdProbeError;
    async fn read_sector(&mut self, lba: u32, out: &mut [u8; 512]) -> Result<(), Self::Error> {
        SdCardProbe::read_sector(self, lba, out).await
    }
    async fn write_sector(&mut self, lba: u32, data: &[u8; 512]) -> Result<(), Self::Error> {
        SdCardProbe::write_sector(self, lba, data).await
    }
    async fn write_sectors_contiguous(&mut self, lba: u32, data: &[u8]) -> Result<(), Self::Error> {
        SdCardProbe::write_sectors_contiguous(self, lba, data).await
    }
    async fn recover_after_timeout(&mut self) {
        SdCardProbe::recover_after_timeout(self);
    }
}
