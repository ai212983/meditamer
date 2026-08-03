use super::{
    panel_lifecycle::PanelPowerState, DelayOps, I2cOps, InkplateHal, InkplateHalError, Ordering,
    PinMode, ProbeStatus, Result, TestPattern, BUZZ_EN, E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BW,
    FRAMEBUFFER_BYTES, FRAMEBUFFER_TAKEN, FRONTLIGHT_EN, GPIO0_ENABLE, INT1_LSM, INT2_LSM,
    INT_APDS, IO_INT_ADDR, PARTIAL_TRANSITION_BYTES, PWRUP, SD_PMOS_PIN, TPS65186_ADDR, VCOM,
    WAKEUP,
};

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
            core::ptr::write_bytes(slot.as_mut_ptr().cast::<u8>(), 0, FRAMEBUFFER_BYTES);
            slot.assume_init_mut()
        };
        Ok(Self {
            i2c,
            delay,
            io_regs_int: [0; 23],
            battery_gate_active_high: None,
            pin_lut,
            panel_fast_ready: false,
            panel_power_state: PanelPowerState::Off,
            framebuffer_bw,
            framebuffer_bw_previous: None,
            partial_transition: None,
            partial_ready: false,
        })
    }

    /// Installs the vendor-sized scratch frame used by partial refreshes.
    /// Without it, partial requests deliberately fall back to a full refresh.
    pub fn install_partial_transition_buffer(&mut self, buffer: &'static mut [u8]) -> bool {
        let Ok(buffer) = <&'static mut [u8; PARTIAL_TRANSITION_BYTES]>::try_from(buffer) else {
            return false;
        };
        self.partial_transition = Some(buffer);
        true
    }

    /// Installs the previous-frame buffer that partial refreshes diff against.
    /// Like the transition buffer, partial requests fall back to a full refresh
    /// while it is absent. Starts cleared so the first partial pass treats the
    /// panel as white.
    pub fn install_previous_framebuffer(&mut self, buffer: &'static mut [u8]) -> bool {
        let Ok(buffer) = <&'static mut [u8; FRAMEBUFFER_BYTES]>::try_from(buffer) else {
            return false;
        };
        buffer.fill(0);
        self.framebuffer_bw_previous = Some(buffer);
        true
    }

    pub fn width(&self) -> usize {
        E_INK_WIDTH
    }

    pub fn height(&self) -> usize {
        E_INK_HEIGHT
    }

    pub async fn probe_devices(&mut self) -> ProbeStatus {
        ProbeStatus {
            io_internal: self.read_i2c_reg(IO_INT_ADDR, 0x00).await.is_ok(),
            io_external: false,
            tps65186: self.read_i2c_reg(TPS65186_ADDR, 0x0F).await.is_ok(),
        }
    }

    pub async fn init_core(&mut self) -> Result<(), I2C::Error> {
        self.io_begin(IO_INT_ADDR).await?;

        self.pin_mode_internal(IO_INT_ADDR, VCOM, PinMode::Output)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, PWRUP, PinMode::Output)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, WAKEUP, PinMode::Output)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, GPIO0_ENABLE, PinMode::Output)
            .await?;
        self.digital_write_internal(IO_INT_ADDR, GPIO0_ENABLE, true)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, INT_APDS, PinMode::InputPullUp)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, INT2_LSM, PinMode::Input)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, INT1_LSM, PinMode::Input)
            .await?;

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)
            .await?;
        embassy_time::Timer::after_millis(1).await;
        self.i2c_write(
            TPS65186_ADDR,
            &[0x09, 0b0001_1011, 0b0000_0000, 0b0001_1011, 0b0000_0000],
        )
        .await?;
        embassy_time::Timer::after_millis(1).await;
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)
            .await?;

        self.pin_mode_internal(IO_INT_ADDR, FRONTLIGHT_EN, PinMode::Output)
            .await?;
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)
            .await?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, true)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, BUZZ_EN, PinMode::Output)
            .await?;
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)
            .await?;
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

    pub(crate) fn framebuffer_bw_mut(&mut self) -> &mut [u8] {
        self.framebuffer_bw
    }

    pub fn draw_test_pattern(&mut self, pattern: TestPattern) {
        self.clear_bw();
        if self.fill_solid_pattern_if_requested(pattern) {
            return;
        }

        for y in 0..E_INK_HEIGHT {
            for x in 0..E_INK_WIDTH {
                if Self::pattern_pixel_is_black(pattern, x, y) {
                    self.set_pixel_bw(x, y, true);
                }
            }
        }
    }

    fn fill_solid_pattern_if_requested(&mut self, pattern: TestPattern) -> bool {
        match pattern {
            TestPattern::SolidBlack => {
                self.framebuffer_bw.fill(0xFF);
                true
            }
            TestPattern::SolidWhite => {
                self.framebuffer_bw.fill(0x00);
                true
            }
            _ => false,
        }
    }

    fn pattern_pixel_is_black(pattern: TestPattern, x: usize, y: usize) -> bool {
        match pattern {
            TestPattern::CheckerboardDiagonals => {
                let checker = ((x / 24) + (y / 24)).is_multiple_of(2);
                let diag_a = x == y;
                let diag_b = x + y == E_INK_WIDTH - 1;
                checker || diag_a || diag_b
            }
            TestPattern::VerticalBars => (x / 50).is_multiple_of(2),
            TestPattern::HorizontalBars => (y / 50).is_multiple_of(2),
            TestPattern::SolidBlack => true,
            TestPattern::SolidWhite => false,
        }
    }
}
