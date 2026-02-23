impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn set_brightness(&mut self, brightness: u8) -> Result<(), I2C::Error> {
        let _ = self.set_brightness_checked(brightness)?;
        Ok(())
    }

    pub fn set_brightness_checked(&mut self, brightness: u8) -> Result<bool, I2C::Error> {
        let mut prep_ok = false;
        for prep_attempt in 0..5u32 {
            let wake_ok = self.set_wakeup(true).is_ok();
            self.delay.delay_ms(2);
            let frontlight_ok = self.frontlight_on().is_ok();
            self.delay.delay_ms(4);
            if wake_ok && frontlight_ok {
                prep_ok = true;
                break;
            }

            let _ = self.frontlight_off();
            let _ = self.i2c.reset();
            self.delay.delay_ms(2 + prep_attempt * 2);
        }
        if !prep_ok {
            return Ok(false);
        }

        let cmd = [0x00, 63u8.saturating_sub(brightness & 0b0011_1111)];
        for attempt in 0..8u32 {
            if self.i2c_write(FRONTLIGHT_DIGIPOT_ADDR, &cmd).is_ok() {
                return Ok(true);
            }
            if attempt == 2 {
                let _ = self.frontlight_off();
                self.delay.delay_ms(3);
                let _ = self.frontlight_on();
                self.delay.delay_ms(5);
            }
            self.delay.delay_ms(2 + attempt * 2);
        }
        Ok(false)
    }

    pub fn frontlight_on(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, true)
    }

    pub fn frontlight_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)
    }

    pub fn buzzer_on(&mut self, freq_hz: i32) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, false)?;
        self.delay.delay_ms(1);
        if self.set_buzzer_frequency(freq_hz).is_err() {
            let _ = self.buzzer_off();
        }
        Ok(())
    }

    pub fn buzzer_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)
    }

    pub fn beep(&mut self, length_ms: u32, freq_hz: i32) -> Result<(), I2C::Error> {
        self.buzzer_on(freq_hz)?;
        self.delay.delay_ms(length_ms);
        self.buzzer_off()
    }

    pub fn read_power_good(&mut self) -> Result<u8, I2C::Error> {
        self.read_i2c_reg(TPS65186_ADDR, 0x0F)
    }

    pub fn set_wakeup(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, enabled)
    }

    pub fn sd_card_power_on(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, false)?;
        self.delay.delay_ms(50);
        Ok(())
    }

    pub fn sd_card_power_off(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Input)
    }

    pub fn touch_power_enabled(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, PinMode::Output)?;
        // Touchscreen power-enable is active-low on Inkplate 4 TEMPERA.
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, !enabled)
    }

    pub fn touch_hardware_reset(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, PinMode::Output)?;
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, false)?;
        self.delay.delay_ms(30);
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, true)?;
        self.delay.delay_ms(30);
        Ok(())
    }

    pub fn touch_software_reset(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.touch_software_reset_read_hello()? == TOUCH_HELLO_PACKET)
    }

    pub fn touch_software_reset_read_hello(&mut self) -> Result<[u8; 4], I2C::Error> {
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_SOFT_RESET_CMD)?;
        let mut hello = [0u8; 4];
        let attempts = TOUCH_SOFT_RESET_TIMEOUT_MS
            .saturating_div(TOUCH_SOFT_RESET_POLL_INTERVAL_MS)
            .max(1);
        for _ in 0..attempts {
            self.delay.delay_ms(TOUCH_SOFT_RESET_POLL_INTERVAL_MS);
            self.i2c_read(TOUCHSCREEN_ADDR, &mut hello)?;
            if hello == TOUCH_HELLO_PACKET {
                break;
            }
        }
        Ok(hello)
    }

    pub fn touch_set_power_state(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        let mut cmd = [0x54, 0x50, 0x00, 0x01];
        if enabled {
            cmd[1] |= 1 << 3;
        }
        self.i2c_write(TOUCHSCREEN_ADDR, &cmd)
    }

    pub fn touch_get_power_state(&mut self) -> Result<bool, I2C::Error> {
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_POWER_STATE_CMD)?;
        let mut response = [0u8; 4];
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        Ok(((response[1] >> 3) & 1) != 0)
    }

    pub fn touch_read_resolution(&mut self) -> Result<(u16, u16), I2C::Error> {
        let mut response = [0u8; 4];

        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_X_RES_CMD)?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        let x_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);

        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_Y_RES_CMD)?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        let y_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);

        Ok((x_res, y_res))
    }

    pub fn touch_init_with_status(&mut self) -> Result<TouchInitStatus, I2C::Error> {
        self.touch_power_enabled(true)?;
        self.delay.delay_ms(180);
        self.touch_hardware_reset()?;

        let mut last_hello = [0u8; 4];
        let mut last_res = (0u16, 0u16);
        for attempt in 0..3u32 {
            let hello = self.touch_software_reset_read_hello()?;
            last_hello = hello;
            if hello != TOUCH_HELLO_PACKET {
                if attempt < 2 {
                    let _ = self.touch_hardware_reset();
                    self.delay.delay_ms(15 + attempt * 10);
                    continue;
                }
                return Ok(TouchInitStatus::HelloMismatch { hello });
            }

            let (x_res, y_res) = self.touch_read_resolution()?;
            last_res = (x_res, y_res);
            if x_res == 0 || y_res == 0 {
                if attempt < 2 {
                    self.delay.delay_ms(10 + attempt * 10);
                    continue;
                }
                return Ok(TouchInitStatus::ZeroResolution { x_res, y_res });
            }

            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
            self.touch_set_power_state(true)?;
            return Ok(TouchInitStatus::Ready { x_res, y_res });
        }

        if last_hello != TOUCH_HELLO_PACKET {
            Ok(TouchInitStatus::HelloMismatch { hello: last_hello })
        } else {
            Ok(TouchInitStatus::ZeroResolution {
                x_res: last_res.0,
                y_res: last_res.1,
            })
        }
    }

    pub fn touch_init(&mut self) -> Result<bool, I2C::Error> {
        Ok(matches!(
            self.touch_init_with_status()?,
            TouchInitStatus::Ready { .. }
        ))
    }

    pub fn touch_shutdown(&mut self) -> Result<(), I2C::Error> {
        let _ = self.touch_set_power_state(false);
        self.touch_power_enabled(false)
    }

    pub fn touch_read_raw_data(&mut self) -> Result<[u8; 8], I2C::Error> {
        let mut raw = [0u8; 8];
        self.i2c_read(TOUCHSCREEN_ADDR, &mut raw)?;
        Ok(raw)
    }

    pub fn touch_read_sample(&mut self, rotation: u8) -> Result<TouchSample, I2C::Error> {
        if self.touch_x_res == 0 || self.touch_y_res == 0 {
            let (x_res, y_res) = self.touch_read_resolution()?;
            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
        }

        let raw = self.touch_read_raw_data()?;
        let bit_count = (raw[7].count_ones() as u8).min(2);
        let mut raw_points = [(0u16, 0u16); 2];
        for (idx, raw_point) in raw_points.iter_mut().enumerate() {
            *raw_point = Self::touch_decode_xy(&raw, idx);
        }
        // Some samples report the active contact in slot 1 while slot 0 is empty.
        // Promote the valid coordinate to slot 0 so higher layers always get a
        // stable primary point for single-touch gesture tracking.
        if raw_points[0] == (0, 0) && raw_points[1] != (0, 0) {
            raw_points.swap(0, 1);
        }
        // Some idle/no-data reads still report non-zero status bits in raw[7].
        // Prefer decoded coordinate validity to avoid phantom touches.
        let coord_count = raw_points
            .iter()
            .filter(|(x, y)| *x != 0 || *y != 0)
            .count() as u8;
        let touch_count = if coord_count == 0 {
            0
        } else {
            bit_count.max(1).min(coord_count)
        };

        let mut points = [TouchPoint::default(); 2];
        for (idx, point) in points.iter_mut().enumerate() {
            let (x_raw, y_raw) = raw_points[idx];
            *point = self.touch_transform_point(x_raw, y_raw, rotation);
        }

        Ok(TouchSample {
            touch_count,
            points,
            raw,
        })
    }

    fn touch_decode_xy(raw: &[u8; 8], index: usize) -> (u16, u16) {
        let base = 1 + index * 3;
        let d0 = raw[base];
        let d1 = raw[base + 1];
        let d2 = raw[base + 2];

        let x = (u16::from(d0 & 0xF0) << 4) | u16::from(d1);
        let y = (u16::from(d0 & 0x0F) << 8) | u16::from(d2);
        (x, y)
    }

    fn touch_scale_axis(raw_value: u16, panel_extent: usize, controller_extent: u16) -> u16 {
        if panel_extent == 0 || controller_extent == 0 {
            return 0;
        }

        let panel_extent_u32 = panel_extent as u32;
        let max_value = panel_extent_u32.saturating_sub(1);
        let numerator = u32::from(raw_value)
            .saturating_mul(panel_extent_u32)
            .saturating_sub(1);
        let scaled = numerator / u32::from(controller_extent);
        scaled.min(max_value) as u16
    }

    fn touch_transform_point(&self, x_raw: u16, y_raw: u16, rotation: u8) -> TouchPoint {
        // Inkplate 4 TEMPERA mapping mirrors both axes at rotation 0.
        let sx = Self::touch_scale_axis(x_raw, E_INK_HEIGHT, self.touch_x_res);
        let sy = Self::touch_scale_axis(y_raw, E_INK_WIDTH, self.touch_y_res);
        let max_x = (E_INK_WIDTH.saturating_sub(1)) as u16;
        let max_y = (E_INK_HEIGHT.saturating_sub(1)) as u16;

        match rotation & 0x03 {
            0 => TouchPoint {
                x: sy,
                y: max_y.saturating_sub(sx),
            },
            1 => TouchPoint {
                x: max_y.saturating_sub(sx),
                y: max_x.saturating_sub(sy),
            },
            2 => TouchPoint {
                x: max_x.saturating_sub(sy),
                y: sx,
            },
            _ => TouchPoint { x: sx, y: sy },
        }
    }

    pub fn fuel_gauge_soc(&mut self) -> Result<u16, I2C::Error> {
        self.wake_fuel_gauge()?;
        self.read_i2c_reg_u16_le(FUEL_GAUGE_ADDR, BQ27441_COMMAND_SOC)
    }

    pub fn lsm6ds3_init_double_tap(&mut self) -> Result<bool, I2C::Error> {
        if self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_WHO_AM_I)? != LSM6DS3_WHO_AM_I_VALUE {
            return Ok(false);
        }

        // SparkFun-style setup: 416Hz accel ODR, +/-2g full scale.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_CTRL1_XL, 0x60])?;
        // Enable gyro at 416Hz / 245dps so app-layer can veto taps during large swings.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_CTRL2_G, 0x60])?;
        // Enable tap detection on X/Y/Z and latch interrupt source until TAP_SRC is read.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_TAP_CFG1, 0x0F])?;
        // Medium threshold: detect enclosure taps, suppress very light contact.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_TAP_THS_6D, 0x09])?;
        // Medium shock/quiet/duration windows.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_INT_DUR2, 0x76])?;
        // Enable single-tap event mode so app-layer can classify multi-tap sequences.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_WAKE_UP_THS, 0x80])?;
        // Route tap and single-tap sources to INT1 (SparkFun reference pattern).
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_MD1_CFG, 0x48])?;

        // Clear any stale latched source.
        let _ = self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_TAP_SRC);
        Ok(true)
    }

    pub fn lsm6ds3_read_tap_src(&mut self) -> Result<u8, I2C::Error> {
        self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_TAP_SRC)
    }

    pub fn lsm6ds3_read_motion_raw(
        &mut self,
    ) -> Result<(i16, i16, i16, i16, i16, i16), I2C::Error> {
        let mut raw = [0u8; 12];
        self.i2c_write_read(LSM6DS3_ADDR, &[LSM6DS3_REG_OUTX_L_G], &mut raw)?;

        let gx = i16::from_le_bytes([raw[0], raw[1]]);
        let gy = i16::from_le_bytes([raw[2], raw[3]]);
        let gz = i16::from_le_bytes([raw[4], raw[5]]);
        let ax = i16::from_le_bytes([raw[6], raw[7]]);
        let ay = i16::from_le_bytes([raw[8], raw[9]]);
        let az = i16::from_le_bytes([raw[10], raw[11]]);
        Ok((gx, gy, gz, ax, ay, az))
    }

    pub fn lsm6ds3_int1_level(&mut self) -> Result<bool, I2C::Error> {
        self.digital_read_internal(IO_INT_ADDR, INT1_LSM)
    }

    pub fn lsm6ds3_int2_level(&mut self) -> Result<bool, I2C::Error> {
        self.digital_read_internal(IO_INT_ADDR, INT2_LSM)
    }

    pub fn lsm6ds3_poll_double_tap(&mut self) -> Result<bool, I2C::Error> {
        let tap_src = self.lsm6ds3_read_tap_src()?;
        Ok((tap_src & LSM6DS3_DOUBLE_TAP_BIT) != 0)
    }

    pub fn lsm6ds3_poll_any_tap(&mut self) -> Result<bool, I2C::Error> {
        let tap_src = self.lsm6ds3_read_tap_src()?;
        Ok((tap_src & LSM6DS3_TAP_EVENT_BIT) != 0)
    }

    pub fn wake_fuel_gauge(&mut self) -> Result<(), I2C::Error> {
        // Inkplate 4 TEMPERA reference wakes BQ27441 via GPOUT pull-up edge.
        self.pin_mode_internal(IO_INT_ADDR, FG_GPOUT, PinMode::InputPullUp)?;
        self.delay.delay_ms(1);
        Ok(())
    }

    pub fn battery_measurement_enable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity()?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, gate_active_high)?;
        self.delay.delay_ms(5);
        Ok(())
    }

    pub fn battery_measurement_disable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity()?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)
    }

    fn detect_battery_gate_polarity(&mut self) -> Result<bool, I2C::Error> {
        if let Some(gate_active_high) = self.battery_gate_active_high {
            return Ok(gate_active_high);
        }

        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Input)?;
        let idle_state_high = self.digital_read_internal(IO_INT_ADDR, BATTERY_MEAS_EN)?;
        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Output)?;

        // Arduino reference uses the level observed while floating to detect board revision.
        // If pin reads low, gate is enabled by driving high on newer revisions.
        let gate_active_high = !idle_state_high;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)?;
        self.battery_gate_active_high = Some(gate_active_high);
        Ok(gate_active_high)
    }

    pub fn debug_snapshot(&mut self) -> Result<DebugSnapshot, I2C::Error> {
        Ok(DebugSnapshot {
            pcal_out0: self.read_i2c_reg(IO_INT_ADDR, 0x02)?,
            pcal_out1: self.read_i2c_reg(IO_INT_ADDR, 0x03)?,
            pcal_cfg0: self.read_i2c_reg(IO_INT_ADDR, 0x06)?,
            pcal_cfg1: self.read_i2c_reg(IO_INT_ADDR, 0x07)?,
        })
    }
}
