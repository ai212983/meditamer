impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn new(i2c: I2C, delay: D) -> Result<Self, I2C::Error> {
        if FRAMEBUFFER_TAKEN.swap(true, Ordering::AcqRel) {
            return Err(InkplateHalError::FramebufferInUse);
        }

        let mut pin_lut = [0u32; 256];
        for (i, slot) in pin_lut.iter_mut().enumerate() {
            let v = i as u8;
            *slot = (((v & 0b0000_0011) as u32) << 4)
                | ((((v & 0b0000_1100) >> 2) as u32) << 18)
                | ((((v & 0b0001_0000) >> 4) as u32) << 23)
                | ((((v & 0b1110_0000) >> 5) as u32) << 25);
        }

        let framebuffer_bw = unsafe {
            let slot = &mut *core::ptr::addr_of_mut!(FRAMEBUFFER_BW);
            core::ptr::write_bytes(slot.as_mut_ptr(), 0, FRAMEBUFFER_BYTES);
            slot.assume_init_mut()
        };
        let previous_bw = unsafe {
            let slot = &mut *core::ptr::addr_of_mut!(PREVIOUS_BW);
            core::ptr::write_bytes(slot.as_mut_ptr(), 0, FRAMEBUFFER_BYTES);
            slot.assume_init_mut()
        };

        Ok(Self {
            i2c,
            delay,
            io_regs_int: [0; 23],
            io_regs_ext: [0; 23],
            battery_gate_active_high: None,
            touch_x_res: 0,
            touch_y_res: 0,
            pin_lut,
            panel_fast_ready: false,
            panel_on: false,
            framebuffer_bw,
            previous_bw,
        })
    }

    pub fn width(&self) -> usize {
        E_INK_WIDTH
    }

    pub fn height(&self) -> usize {
        E_INK_HEIGHT
    }

    pub fn probe_devices(&mut self) -> ProbeStatus {
        ProbeStatus {
            io_internal: self.read_i2c_reg(IO_INT_ADDR, 0x00).is_ok(),
            io_external: self.read_i2c_reg(IO_EXT_ADDR, 0x00).is_ok(),
            tps65186: self.read_i2c_reg(TPS65186_ADDR, 0x0F).is_ok(),
        }
    }

    pub fn init_core(&mut self) -> Result<(), I2C::Error> {
        self.io_begin(IO_INT_ADDR)?;
        self.io_begin(IO_EXT_ADDR)?;

        self.pin_mode_internal(IO_INT_ADDR, VCOM, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, PWRUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, WAKEUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GPIO0_ENABLE, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, GPIO0_ENABLE, true)?;
        self.pin_mode_internal(IO_INT_ADDR, INT_APDS, PinMode::InputPullUp)?;
        self.pin_mode_internal(IO_INT_ADDR, INT2_LSM, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, INT1_LSM, PinMode::Input)?;

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        self.delay.delay_ms(1);
        self.i2c_write(
            TPS65186_ADDR,
            &[0x09, 0b0001_1011, 0b0000_0000, 0b0001_1011, 0b0000_0000],
        )?;
        self.delay.delay_ms(1);
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;

        self.pin_mode_internal(IO_INT_ADDR, FRONTLIGHT_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)?;
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, true)?;
        self.pin_mode_internal(IO_INT_ADDR, BUZZ_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)?;
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, PinMode::Output)?;
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, PinMode::Output)?;
        // Touchscreen power-enable is active-low; keep it off by default.
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, true)?;
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, true)?;

        Ok(())
    }

    pub fn clear_bw(&mut self) {
        self.framebuffer_bw.fill(0);
    }

    pub fn set_pixel_bw(&mut self, x: usize, y: usize, black: bool) {
        if x >= E_INK_WIDTH || y >= E_INK_HEIGHT {
            return;
        }

        // Panel scan order is currently 90deg CCW from logical coordinates.
        // Rotate logical pixels CW into the backing buffer to compensate.
        let panel_x = E_INK_WIDTH - 1 - y;
        let panel_y = x;

        let byte_idx = (E_INK_WIDTH / 8) * panel_y + (panel_x / 8);
        let bit = 1u8 << (panel_x % 8);
        if black {
            self.framebuffer_bw[byte_idx] |= bit;
        } else {
            self.framebuffer_bw[byte_idx] &= !bit;
        }
    }

    pub fn draw_test_pattern(&mut self, pattern: TestPattern) {
        self.clear_bw();
        match pattern {
            TestPattern::CheckerboardDiagonals => {
                for y in 0..E_INK_HEIGHT {
                    for x in 0..E_INK_WIDTH {
                        let checker = ((x / 24) + (y / 24)) % 2 == 0;
                        let diag_a = x == y;
                        let diag_b = x + y == E_INK_WIDTH - 1;
                        if checker || diag_a || diag_b {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::VerticalBars => {
                for y in 0..E_INK_HEIGHT {
                    for x in 0..E_INK_WIDTH {
                        if (x / 50) % 2 == 0 {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::HorizontalBars => {
                for y in 0..E_INK_HEIGHT {
                    if (y / 50) % 2 == 0 {
                        for x in 0..E_INK_WIDTH {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::SolidBlack => self.framebuffer_bw.fill(0xFF),
            TestPattern::SolidWhite => self.framebuffer_bw.fill(0x00),
        }
    }
}
