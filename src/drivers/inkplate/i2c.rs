use super::*;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    fn cached_regs_mut(&mut self, addr: u8) -> Result<&mut [u8; 23], I2C::Error> {
        match addr {
            IO_INT_ADDR => Ok(&mut self.io_regs_int),
            IO_EXT_ADDR => Ok(&mut self.io_regs_ext),
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    fn cached_regs(&self, addr: u8) -> Result<&[u8; 23], I2C::Error> {
        match addr {
            IO_INT_ADDR => Ok(&self.io_regs_int),
            IO_EXT_ADDR => Ok(&self.io_regs_ext),
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    pub(super) fn io_begin(&mut self, addr: u8) -> Result<(), I2C::Error> {
        let mut regs = [0u8; 23];
        self.i2c_write_read(addr, &[0x00], &mut regs)?;
        match addr {
            IO_INT_ADDR => {
                self.io_regs_int = regs;
                Ok(())
            }
            IO_EXT_ADDR => {
                self.io_regs_ext = regs;
                Ok(())
            }
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    pub(super) fn pin_mode_internal(
        &mut self,
        addr: u8,
        pin: u8,
        mode: PinMode,
    ) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let cfg_idx = if port == 0 {
            PCAL_CFGPORT0_ARRAY
        } else {
            PCAL_CFGPORT1_ARRAY
        };
        let out_idx = if port == 0 {
            PCAL_OUTPORT0_ARRAY
        } else {
            PCAL_OUTPORT1_ARRAY
        };
        let pupden_idx = if port == 0 {
            PCAL_PUPDEN_REG0_ARRAY
        } else {
            PCAL_PUPDEN_REG1_ARRAY
        };
        let pupdsel_idx = if port == 0 {
            PCAL_PUPDSEL_REG0_ARRAY
        } else {
            PCAL_PUPDSEL_REG1_ARRAY
        };

        match mode {
            PinMode::Input => {
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.write_cached_reg(addr, cfg_idx)?;
            }
            PinMode::Output => {
                self.modify_cached_reg(addr, cfg_idx, 0, 1u8 << bit)?;
                self.modify_cached_reg(addr, out_idx, 0, 1u8 << bit)?;
                self.write_cached_reg(addr, out_idx)?;
                self.write_cached_reg(addr, cfg_idx)?;
            }
            PinMode::InputPullUp => {
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupden_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupdsel_idx, 1u8 << bit, 0)?;
                self.write_cached_reg(addr, cfg_idx)?;
                self.write_cached_reg(addr, pupden_idx)?;
                self.write_cached_reg(addr, pupdsel_idx)?;
            }
            PinMode::InputPullDown => {
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupden_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupdsel_idx, 0, 1u8 << bit)?;
                self.write_cached_reg(addr, cfg_idx)?;
                self.write_cached_reg(addr, pupden_idx)?;
                self.write_cached_reg(addr, pupdsel_idx)?;
            }
        }
        Ok(())
    }

    pub(super) fn digital_write_internal(
        &mut self,
        addr: u8,
        pin: u8,
        state: bool,
    ) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let out_idx = if port == 0 {
            PCAL_OUTPORT0_ARRAY
        } else {
            PCAL_OUTPORT1_ARRAY
        };
        if state {
            self.modify_cached_reg(addr, out_idx, 1u8 << bit, 0)?;
        } else {
            self.modify_cached_reg(addr, out_idx, 0, 1u8 << bit)?;
        }
        self.write_cached_reg(addr, out_idx)?;
        Ok(())
    }

    pub(super) fn digital_read_internal(&mut self, addr: u8, pin: u8) -> Result<bool, I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let in_idx = if port == 0 { 0 } else { 1 };
        let value = self.read_i2c_reg(addr, PCAL_REG_ADDRS[in_idx])?;
        Ok((value & (1u8 << bit)) != 0)
    }

    fn modify_cached_reg(
        &mut self,
        addr: u8,
        idx: usize,
        set_mask: u8,
        clear_mask: u8,
    ) -> Result<(), I2C::Error> {
        let regs = self.cached_regs_mut(addr)?;
        regs[idx] |= set_mask;
        regs[idx] &= !clear_mask;
        Ok(())
    }

    fn write_cached_reg(&mut self, addr: u8, idx: usize) -> Result<(), I2C::Error> {
        let reg_value = self.cached_regs(addr)?[idx];
        self.i2c_write(addr, &[PCAL_REG_ADDRS[idx], reg_value])
    }

    pub(super) fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        match self.i2c.write(addr, bytes) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c.write(addr, bytes).map_err(InkplateHalError::I2c)
            }
        }
    }

    pub(super) fn i2c_write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2C::Error> {
        match self.i2c.write_read(addr, bytes, buffer) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c
                    .write_read(addr, bytes, buffer)
                    .map_err(InkplateHalError::I2c)
            }
        }
    }

    pub(super) fn i2c_read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), I2C::Error> {
        match self.i2c.read(addr, buffer) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c.read(addr, buffer).map_err(InkplateHalError::I2c)
            }
        }
    }

    pub(super) fn read_i2c_reg(&mut self, addr: u8, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf = [0u8; 1];
        self.i2c_write_read(addr, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    pub(super) fn read_i2c_reg_u16_le(&mut self, addr: u8, reg: u8) -> Result<u16, I2C::Error> {
        let mut buf = [0u8; 2];
        self.i2c_write_read(addr, &[reg], &mut buf)?;
        Ok(((buf[1] as u16) << 8) | (buf[0] as u16))
    }

    pub fn i2c_fault_recovery_smoke(&mut self, attempts: u8) -> Result<(), I2C::Error> {
        let tries = attempts.max(1);
        for _ in 0..tries {
            // 0x7F is intentionally unused in this design; this forces an address NACK.
            let _ = self.i2c_write(0x7F, &[0x00]);
            self.delay.delay_ms(1);
        }

        let _ = self.read_i2c_reg(IO_INT_ADDR, 0x00)?;
        Ok(())
    }

    pub(super) fn set_buzzer_frequency(&mut self, freq_hz: i32) -> Result<(), I2C::Error> {
        if !ENABLE_BUZZER_PITCH_CONTROL {
            return Ok(());
        }
        let constrained = freq_hz.clamp(BEEP_FREQ_MIN_HZ, BEEP_FREQ_MAX_HZ);
        let wiper_percent = 156.499_57f32 + (-0.130_347_34f32 * constrained as f32);
        if !(0.0..=100.0).contains(&wiper_percent) {
            return Ok(());
        }
        let wiper_value = (((wiper_percent / 100.0) * 127.0) + 0.5) as u8;
        let payload = [wiper_value & 0x7F];

        for _ in 0..4 {
            if self.i2c_write(BUZZER_DIGIPOT_ADDR, &payload).is_ok() {
                return Ok(());
            }
            let _ = self.frontlight_on();
            let _ = self.i2c.reset();
            self.delay.delay_ms(2);
        }
        Ok(())
    }
}
