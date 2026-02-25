use super::*;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub(super) fn clean(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start()?;
            for _ in 0..E_INK_HEIGHT {
                self.hscan_start(send);
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(CL_MASK);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(DATA_MASK | CL_MASK);
                self.vscan_end();
            }
            self.delay.delay_us(230);
        }
        Ok(())
    }

    pub(super) async fn clean_async(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start()?;
            for row in 0..E_INK_HEIGHT {
                self.hscan_start(send);
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(CL_MASK);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(DATA_MASK | CL_MASK);
                self.vscan_end();

                if (row & 0x1F) == 0 {
                    embassy_time::Timer::after_micros(0).await;
                }
            }
            embassy_time::Timer::after_micros(230).await;
        }
        Ok(())
    }
}
