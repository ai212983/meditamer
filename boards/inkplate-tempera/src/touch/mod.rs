// Mixed, not hardware-only: `elan` also carries the pure decode/transform
// functions the host tests below exercise, so its module declaration stays
// ungated even though its `InkplateTouch` impl is `cfg(target_os = "none")`.
mod elan;
mod power;
mod protocol;

pub use protocol::{active_slots, is_touch_report};

#[cfg(target_os = "none")]
#[derive(Clone, Copy, Default)]
pub struct TouchRetryTiming {
    pub retries: u8,
    pub wait_us: u32,
}

use crate::{adapters::I2cOps, InkplateHalError, Result};

pub struct InkplateTouch<I2C> {
    i2c: I2C,
    io_regs_ext: [u8; 23],
    external_ready: bool,
    #[cfg(target_os = "none")]
    touch_x_res: u16,
    #[cfg(target_os = "none")]
    touch_y_res: u16,
}

impl<I2C> InkplateTouch<I2C>
where
    I2C: I2cOps,
{
    pub const fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            io_regs_ext: [0; 23],
            external_ready: false,
            #[cfg(target_os = "none")]
            touch_x_res: 0,
            #[cfg(target_os = "none")]
            touch_y_res: 0,
        }
    }

    pub async fn probe_external(&mut self) -> bool {
        self.i2c.probe(IO_EXT_ADDR).await.unwrap_or(false)
    }

    pub async fn probe_controller(&mut self) -> bool {
        self.i2c.probe(TOUCHSCREEN_ADDR).await.unwrap_or(false)
    }

    async fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        match self.i2c.write(addr, bytes).await {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset().await;
                embassy_time::Timer::after_millis(1).await;
                self.i2c
                    .write(addr, bytes)
                    .await
                    .map_err(InkplateHalError::I2c)
            }
        }
    }

    #[cfg(target_os = "none")]
    async fn i2c_read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), I2C::Error> {
        match self.i2c.read(addr, buffer).await {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset().await;
                embassy_time::Timer::after_millis(1).await;
                self.i2c
                    .read(addr, buffer)
                    .await
                    .map_err(InkplateHalError::I2c)
            }
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) const IO_EXT_ADDR: u8 = 0x21;

pub(super) const TOUCHSCREEN_ADDR: u8 = 0x15;

pub(super) const TOUCHSCREEN_EN: u8 = 0;

#[cfg(target_os = "none")]
pub(super) const TOUCHSCREEN_RST: u8 = 1;
