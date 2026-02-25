use super::*;

mod sensors;
mod touch;

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
}

fn touch_raw_frame_has_contact(raw: &[u8; 8], x_res: u16, y_res: u16) -> bool {
    if raw[7].count_ones() > 0 {
        return true;
    }
    for idx in 0..2 {
        let base = 1 + idx * 3;
        let d0 = raw[base];
        let d1 = raw[base + 1];
        let d2 = raw[base + 2];
        let x_raw = (u16::from(d0 & 0xF0) << 4) | u16::from(d1);
        let y_raw = (u16::from(d0 & 0x0F) << 8) | u16::from(d2);
        if touch_raw_point_plausible(x_raw, y_raw, x_res, y_res) {
            return true;
        }
    }
    false
}

fn touch_raw_point_plausible(x_raw: u16, y_raw: u16, x_res: u16, y_res: u16) -> bool {
    if x_raw == 0 || y_raw == 0 {
        return false;
    }
    if x_res == 0 || y_res == 0 {
        return false;
    }
    // Controller occasionally emits out-of-range garbage while idle.
    // Clamp-to-edge transforms of those samples create phantom touches.
    x_raw <= x_res && y_raw <= y_res
}

const TOUCH_RAW_EMPTY_RETRY_COUNT: u8 = 2;
const TOUCH_RAW_EMPTY_RETRY_DELAY_MS: u32 = 1;

fn touch_presence_count(bit_count: u8, coord_count: u8) -> u8 {
    // Reference stack keeps contact presence from status bits. Keep that signal
    // so coordinate dropouts do not truncate an active swipe.
    bit_count.max(coord_count).min(2)
}

#[cfg(test)]
mod tests;
