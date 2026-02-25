use esp_hal::{
    i2c::master::{Error as I2cError, I2c},
    time::{Duration, Instant},
    Blocking,
};

pub trait DelayOps {
    fn delay_us(&self, micros: u32);
    fn delay_ms(&self, millis: u32);
}

pub trait I2cOps {
    type Error;

    fn read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), Self::Error>;
    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error>;
    fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Self::Error>;
    fn probe(&mut self, addr: u8) -> Result<bool, Self::Error>;
    fn reset(&mut self) -> Result<(), Self::Error>;
}

pub struct HalI2c<'d> {
    bus: I2c<'d, Blocking>,
}

impl<'d> HalI2c<'d> {
    pub fn new(bus: I2c<'d, Blocking>) -> Self {
        Self { bus }
    }
}

impl I2cOps for HalI2c<'_> {
    type Error = I2cError;

    fn read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.read(addr, buffer)
    }

    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), Self::Error> {
        self.bus.write(addr, bytes)
    }

    fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.bus.write_read(addr, bytes, buffer)
    }

    fn probe(&mut self, addr: u8) -> Result<bool, Self::Error> {
        match self.bus.write(addr, &[0x00]) {
            Ok(()) => Ok(true),
            Err(I2cError::AcknowledgeCheckFailed(_)) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn reset(&mut self) -> Result<(), Self::Error> {
        // `esp-hal` resets the peripheral state on each transaction path.
        // Keep the trait hook for parity with ESP-IDF migration behavior.
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
