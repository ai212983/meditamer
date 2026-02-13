impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn prepare_panel_fast_io(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Output)?;

        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;

        GpioFast::out_enable_set(PANEL_OUT_ENABLE_MASK);
        GpioFast::out_enable1_set(PANEL_OUT1_ENABLE_MASK);
        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(true);

        self.panel_fast_ready = true;
        Ok(())
    }

    pub fn panel_fast_waveform_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        let word_a = self.pin_lut[0xAA];
        let word_5 = self.pin_lut[0x55];
        for _ in 0..8 {
            self.write_data_and_clock(word_a);
            self.write_data_and_clock(word_5);
        }
        self.set_le(true);
        self.set_le(false);
        self.set_ckv(true);
        self.set_ckv(false);
        Ok(())
    }

    pub fn panel_waveform_primitives_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        self.vscan_start()?;
        for _ in 0..8 {
            self.hscan_start(self.pin_lut[0xAA]);
            self.pulse_cl_only();
            self.pulse_cl_only();
            self.vscan_end();
        }
        self.set_ckv(false);
        self.set_le(false);
        self.set_sph(true);
        self.set_cl(false);
        Ok(())
    }

    pub fn panel_clean_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        let send = self.pin_lut[0b1010_1010];
        self.vscan_start()?;
        for _ in 0..8 {
            self.hscan_start(send);
            GpioFast::out_set(send | CL_MASK);
            GpioFast::out_clear(DATA_MASK | CL_MASK);
            for _ in 0..8 {
                self.pulse_cl_only();
                self.pulse_cl_only();
            }
            GpioFast::out_set(send | CL_MASK);
            GpioFast::out_clear(DATA_MASK | CL_MASK);
            self.vscan_end();
        }
        self.delay.delay_us(230);
        self.set_ckv(false);
        self.set_le(false);
        self.set_sph(true);
        self.set_cl(false);
        Ok(())
    }

    pub fn eink_on(&mut self) -> Result<(), I2C::Error> {
        if self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        self.delay.delay_ms(5);

        self.i2c_write(TPS65186_ADDR, &[0x01, 0b0010_0000])?;
        self.i2c_write(TPS65186_ADDR, &[0x09, 0b1110_0100])?;
        self.i2c_write(TPS65186_ADDR, &[0x0B, 0b0001_1011])?;

        self.prepare_panel_fast_io()?;
        self.set_le(false);
        self.set_cl(false);
        self.set_sph(true);
        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.set_ckv(false);
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, true)?;

        let mut ok = false;
        let mut last_pg = 0u8;
        for _ in 0..250 {
            self.delay.delay_ms(1);
            last_pg = self.read_power_good()?;
            if last_pg == PWR_GOOD_OK {
                ok = true;
                break;
            }
        }
        if !ok {
            let _ = self.eink_off();
            return Err(InkplateHalError::PanelPowerTimeout(last_pg));
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, true)?;
        self.panel_on = true;
        Ok(())
    }

    pub fn eink_off(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, false)?;
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, GMOD, false)?;

        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(false);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, false)?;

        for _ in 0..250 {
            self.delay.delay_ms(1);
            if self.read_power_good()? == 0 {
                break;
            }
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;
        self.i2c_write(TPS65186_ADDR, &[0x01, 0x00])?;
        self.panel_on = false;
        Ok(())
    }
}
