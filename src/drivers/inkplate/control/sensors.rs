use super::*;

type Lsm6ds3MotionRaw = (i16, i16, i16, i16, i16, i16);

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
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

    pub fn lsm6ds3_read_motion_raw(&mut self) -> Result<Lsm6ds3MotionRaw, I2C::Error> {
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
