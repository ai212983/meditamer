use super::{
    DelayOps, I2cOps, InkplateHal, PinMode, Result, BUZZ_EN, FRONTLIGHT_DIGIPOT_ADDR,
    FRONTLIGHT_EN, IO_INT_ADDR, SD_PMOS_PIN, TPS65186_ADDR, WAKEUP,
};

mod sensors;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub async fn set_brightness(&mut self, brightness: u8) -> Result<(), I2C::Error> {
        let _ = self.set_brightness_checked(brightness).await?;
        Ok(())
    }

    pub async fn set_brightness_checked(&mut self, brightness: u8) -> Result<bool, I2C::Error> {
        let mut prep_ok = false;
        for prep_attempt in 0..5u32 {
            let wake_ok = self.set_wakeup(true).await.is_ok();
            embassy_time::Timer::after_millis(2).await;
            let frontlight_ok = self.frontlight_on().await.is_ok();
            embassy_time::Timer::after_millis(4).await;
            if wake_ok && frontlight_ok {
                prep_ok = true;
                break;
            }

            let _ = self.frontlight_off().await;
            let _ = self.i2c.reset().await;
            embassy_time::Timer::after_millis((2 + prep_attempt * 2) as u64).await;
        }
        if !prep_ok {
            return Ok(false);
        }

        let cmd = [0x00, 63u8.saturating_sub(brightness & 0b0011_1111)];
        for attempt in 0..8u32 {
            if self.i2c_write(FRONTLIGHT_DIGIPOT_ADDR, &cmd).await.is_ok() {
                return Ok(true);
            }
            if attempt == 2 {
                let _ = self.frontlight_off().await;
                embassy_time::Timer::after_millis(3).await;
                let _ = self.frontlight_on().await;
                embassy_time::Timer::after_millis(5).await;
            }
            embassy_time::Timer::after_millis((2 + attempt * 2) as u64).await;
        }
        Ok(false)
    }

    pub async fn frontlight_on(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, true)
            .await
    }

    pub async fn frontlight_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)
            .await
    }

    pub async fn buzzer_on(&mut self, freq_hz: i32) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, false)
            .await?;
        embassy_time::Timer::after_millis(1).await;
        if self.set_buzzer_frequency(freq_hz).await.is_err() {
            let _ = self.buzzer_off().await;
        }
        Ok(())
    }

    pub async fn buzzer_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)
            .await
    }

    pub async fn beep(&mut self, length_ms: u32, freq_hz: i32) -> Result<(), I2C::Error> {
        self.buzzer_on(freq_hz).await?;
        embassy_time::Timer::after_millis(length_ms as u64).await;
        self.buzzer_off().await
    }

    pub async fn read_power_good(&mut self) -> Result<u8, I2C::Error> {
        self.read_i2c_reg(TPS65186_ADDR, 0x0F).await
    }

    pub async fn set_wakeup(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, enabled)
            .await
    }

    pub async fn sd_card_power_on(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)
            .await?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, false)
            .await?;
        embassy_time::Timer::after_millis(50).await;
        Ok(())
    }

    pub async fn sd_card_power_off(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Input)
            .await
    }
}
