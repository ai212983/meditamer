//! Error-only startup tracing inside the existing shared-bus lock.
//!
//! No probes, retries or recovery are added. The startup owner disables the
//! sink before launching runtime clients; provider diagnostics then take over.
//! Async calls await the underlying transport while preserving transaction
//! boundaries and the original error. Tracing adds no bus acquisition.

use embassy_embedded_hal::SetConfig;
use embedded_hal::i2c::{ErrorType, I2c, Operation};

pub trait Diagnostics<E> {
    fn failed(&mut self, address: u8, operations: &[Operation<'_>], error: &E);
}

pub struct StartupI2c<I, D> {
    inner: I,
    diagnostics: Option<D>,
}

impl<I, D> StartupI2c<I, D> {
    pub const fn new(inner: I, diagnostics: D) -> Self {
        Self {
            inner,
            diagnostics: Some(diagnostics),
        }
    }

    pub fn finish_startup(&mut self) {
        self.diagnostics = None;
    }
}

impl<I: ErrorType, D> ErrorType for StartupI2c<I, D> {
    type Error = I::Error;
}

impl<I: SetConfig, D> SetConfig for StartupI2c<I, D> {
    type Config = I::Config;
    type ConfigError = I::ConfigError;

    fn set_config(&mut self, config: &Self::Config) -> Result<(), Self::ConfigError> {
        self.inner.set_config(config)
    }
}

impl<I: I2c, D: Diagnostics<I::Error>> I2c for StartupI2c<I, D> {
    fn read(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let result = self.inner.read(address, bytes);
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, &[Operation::Read(bytes)], error);
        }
        result
    }

    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        let result = self.inner.write(address, bytes);
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, &[Operation::Write(bytes)], error);
        }
        result
    }

    fn write_read(
        &mut self,
        address: u8,
        written: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        let result = self.inner.write_read(address, written, read);
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(
                address,
                &[Operation::Write(written), Operation::Read(read)],
                error,
            );
        }
        result
    }

    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        let result = self.inner.transaction(address, operations);
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, operations, error);
        }
        result
    }
}

impl<I: embedded_hal_async::i2c::I2c, D: Diagnostics<I::Error>> embedded_hal_async::i2c::I2c
    for StartupI2c<I, D>
{
    async fn read(&mut self, address: u8, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let result = embedded_hal_async::i2c::I2c::read(&mut self.inner, address, bytes).await;
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, &[Operation::Read(bytes)], error);
        }
        result
    }

    async fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        let result = embedded_hal_async::i2c::I2c::write(&mut self.inner, address, bytes).await;
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, &[Operation::Write(bytes)], error);
        }
        result
    }

    async fn write_read(
        &mut self,
        address: u8,
        written: &[u8],
        read: &mut [u8],
    ) -> Result<(), Self::Error> {
        let result =
            embedded_hal_async::i2c::I2c::write_read(&mut self.inner, address, written, read).await;
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(
                address,
                &[Operation::Write(written), Operation::Read(read)],
                error,
            );
        }
        result
    }

    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        let result =
            embedded_hal_async::i2c::I2c::transaction(&mut self.inner, address, operations).await;
        if let (Err(error), Some(diagnostics)) = (&result, &mut self.diagnostics) {
            diagnostics.failed(address, operations, error);
        }
        result
    }
}

#[cfg(test)]
mod tests;
