impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    fn vscan_start(&mut self) -> Result<(), I2C::Error> {
        self.set_ckv(true);
        self.delay.delay_us(7);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(8);
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(18);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(18);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        Ok(())
    }

    fn vscan_end(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
    }

    fn hscan_start(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    fn write_data_and_clock(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    fn pulse_cl_only(&self) {
        GpioFast::out_set(CL_MASK);
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    fn clear_data_and_cl_le(&self) {
        GpioFast::out_clear(DATA_MASK | LE_MASK | CL_MASK);
    }

    #[inline(always)]
    fn set_le(&self, high: bool) {
        if high {
            GpioFast::out_set(LE_MASK);
        } else {
            GpioFast::out_clear(LE_MASK);
        }
    }

    #[inline(always)]
    fn set_cl(&self, high: bool) {
        if high {
            GpioFast::out_set(CL_MASK);
        } else {
            GpioFast::out_clear(CL_MASK);
        }
    }

    #[inline(always)]
    fn set_ckv(&self, high: bool) {
        if high {
            GpioFast::out1_set(CKV_MASK1);
        } else {
            GpioFast::out1_clear(CKV_MASK1);
        }
    }

    #[inline(always)]
    fn set_sph(&self, high: bool) {
        if high {
            GpioFast::out1_set(SPH_MASK1);
        } else {
            GpioFast::out1_clear(SPH_MASK1);
        }
    }
}
