use super::*;
use crate::drivers::inkplate::{
    IO_EXT_ADDR, PCAL_CFGPORT0_ARRAY, PCAL_OUTPORT0_ARRAY, PCAL_REG_ADDRS, TOUCHSCREEN_EN,
    TOUCHSCREEN_RST,
};

impl<I2C> InkplateTouch<I2C>
where
    I2C: I2cOps,
{
    async fn initialize_external_expander(&mut self) -> Result<(), I2C::Error> {
        if self.external_ready {
            return Ok(());
        }
        let mut regs = [0u8; 23];
        self.i2c
            .write_read(IO_EXT_ADDR, &[0x00], &mut regs)
            .await
            .map_err(InkplateHalError::I2c)?;
        self.io_regs_ext = regs;
        self.external_ready = true;
        Ok(())
    }

    async fn set_output(&mut self, pin: u8, high: bool) -> Result<(), I2C::Error> {
        self.initialize_external_expander().await?;
        let bit = 1u8 << pin;
        self.io_regs_ext[PCAL_CFGPORT0_ARRAY] &= !bit;
        if high {
            self.io_regs_ext[PCAL_OUTPORT0_ARRAY] |= bit;
        } else {
            self.io_regs_ext[PCAL_OUTPORT0_ARRAY] &= !bit;
        }
        self.i2c_write(
            IO_EXT_ADDR,
            &[
                PCAL_REG_ADDRS[PCAL_OUTPORT0_ARRAY],
                self.io_regs_ext[PCAL_OUTPORT0_ARRAY],
            ],
        )
        .await?;
        self.i2c_write(
            IO_EXT_ADDR,
            &[
                PCAL_REG_ADDRS[PCAL_CFGPORT0_ARRAY],
                self.io_regs_ext[PCAL_CFGPORT0_ARRAY],
            ],
        )
        .await
    }

    pub async fn set_power_enabled(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        // Touchscreen power-enable is active-low on Inkplate 4 TEMPERA.
        self.set_output(TOUCHSCREEN_EN, !enabled).await
    }

    pub(super) async fn hardware_reset(&mut self) -> Result<(), I2C::Error> {
        self.set_output(TOUCHSCREEN_RST, false).await?;
        embassy_time::Timer::after_millis(30).await;
        self.set_output(TOUCHSCREEN_RST, true).await?;
        embassy_time::Timer::after_millis(30).await;
        Ok(())
    }
}
