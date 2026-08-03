use super::{
    DelayOps, GpioFast, I2cOps, InkplateHal, Result, CL_MASK, DATA_MASK, E_INK_HEIGHT, E_INK_WIDTH,
};
use esp_sync::raw::{RawLock, SingleCoreInterruptLock};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
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
            self.vscan_start().await?;
            self.scan_clean_pass(send);
            self.delay.delay_us(230);
        }
        Ok(())
    }

    /// A complete cleanup frame is one timing transaction. It runs from IRAM
    /// with interrupts masked so unrelated firmware activity cannot stretch a
    /// CKV/LE row boundary and produce horizontal bands.
    #[esp_hal::ram]
    fn scan_clean_pass(&self, send: u32) {
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

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

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }
}
