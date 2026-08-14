use embedded_hal_async::i2c::I2c;
use esp_hal::time::{Duration, Instant};

pub trait DelayOps {
    fn delay_us(&self, micros: u32);
    fn delay_ms(&self, millis: u32);
}

#[allow(async_fn_in_trait)]
pub trait I2cOps {
    type Error;

    async fn read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), Self::Error>;
    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error>;
    async fn write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error>;
    async fn probe(&mut self, addr: u8) -> Result<bool, Self::Error>;
    async fn reset(&mut self) -> Result<(), Self::Error>;
}

impl<T> I2cOps for T
where
    T: I2c,
{
    type Error = T::Error;

    async fn read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        I2c::read(self, addr, buffer).await
    }

    async fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        I2c::write(self, addr, bytes).await
    }

    async fn write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        I2c::write_read(self, addr, bytes, buffer).await
    }

    async fn probe(&mut self, addr: u8) -> Result<bool, Self::Error> {
        Ok(I2c::write(self, addr, &[0x00]).await.is_ok())
    }

    async fn reset(&mut self) -> Result<(), Self::Error> {
        // esp-hal resets peripheral state on transaction paths. Retain the
        // hook so device drivers can use the same recovery sequence.
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
pub struct BusyDelay;

impl BusyDelay {
    pub const fn new() -> Self {
        Self
    }

    fn delay_duration(&self, duration: Duration) {
        let start = Instant::now();
        while start.elapsed() < duration {}
    }
}

impl DelayOps for BusyDelay {
    fn delay_us(&self, micros: u32) {
        self.delay_duration(Duration::from_micros(micros as u64));
    }

    fn delay_ms(&self, millis: u32) {
        self.delay_duration(Duration::from_millis(millis as u64));
    }
}
