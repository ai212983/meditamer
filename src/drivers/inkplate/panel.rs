use super::{
    DelayOps, GpioFast, I2cOps, InkplateHal, InkplateHalError, PinMode, Result, CL_MASK, DATA_MASK,
    GMOD, IO_INT_ADDR, OE, PANEL_OUT1_ENABLE_MASK, PANEL_OUT_ENABLE_MASK, PWRUP, PWR_GOOD_OK, SPV,
    TPS65186_ADDR, VCOM, WAKEUP,
};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub async fn prepare_panel_fast_io(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Output)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Output)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Output)
            .await?;

        self.digital_write_internal(IO_INT_ADDR, GMOD, true).await?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true).await?;
        self.digital_write_internal(IO_INT_ADDR, OE, false).await?;

        GpioFast::out_enable_set(PANEL_OUT_ENABLE_MASK);
        GpioFast::out_enable1_set(PANEL_OUT1_ENABLE_MASK);
        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(true);

        self.panel_fast_ready = true;
        Ok(())
    }

    pub async fn panel_fast_waveform_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io().await?;
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

    pub async fn panel_waveform_primitives_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io().await?;
        }
        self.vscan_start().await?;
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

    pub async fn panel_clean_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io().await?;
        }
        let send = self.pin_lut[0b1010_1010];
        self.vscan_start().await?;
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

    pub async fn eink_on_async(&mut self) -> Result<(), I2C::Error> {
        if self.panel_power_state.is_on() {
            return Ok(());
        }

        // `Starting` means a previous startup or shutdown did not reach a
        // confirmed clean state. Retry shutdown before energizing any rail.
        if self.panel_power_state.requires_shutdown() {
            self.eink_off_async().await?;
        }

        // Set this before the first hardware side effect. From this point on,
        // every error path must attempt shutdown, even if the PMIC never
        // reached its fully-on state.
        self.panel_power_state.begin_startup();
        let startup = self.eink_on_sequence_async().await;
        match startup {
            Ok(()) => {
                self.panel_power_state.startup_succeeded();
                Ok(())
            }
            Err(error) => {
                // Preserve the startup error. eink_off_async retains
                // `Starting` if recovery itself fails, causing the next call
                // to retry shutdown before another startup.
                let _shutdown = self.eink_off_async().await;
                Err(error)
            }
        }
    }

    async fn eink_on_sequence_async(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)
            .await?;
        embassy_time::Timer::after_millis(5).await;

        self.i2c_write(TPS65186_ADDR, &[0x01, 0b0010_0000]).await?;
        self.i2c_write(TPS65186_ADDR, &[0x09, 0b1110_0100]).await?;
        self.i2c_write(TPS65186_ADDR, &[0x0B, 0b0001_1011]).await?;

        self.prepare_panel_fast_io().await?;
        self.set_le(false);
        self.set_cl(false);
        self.set_sph(true);
        self.digital_write_internal(IO_INT_ADDR, GMOD, true).await?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true).await?;
        self.set_ckv(false);
        self.digital_write_internal(IO_INT_ADDR, OE, false).await?;

        self.digital_write_internal(IO_INT_ADDR, PWRUP, true)
            .await?;

        let mut ok = false;
        let mut last_pg = 0u8;
        for _ in 0..250 {
            embassy_time::Timer::after_millis(1).await;
            last_pg = self.read_power_good().await?;
            if last_pg == PWR_GOOD_OK {
                ok = true;
                break;
            }
        }
        if !ok {
            return Err(InkplateHalError::PanelPowerTimeout(last_pg));
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, true).await?;
        self.digital_write_internal(IO_INT_ADDR, OE, true).await?;
        Ok(())
    }

    pub async fn eink_off_async(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_power_state.requires_shutdown() {
            return Ok(());
        }

        // Power-down is best-effort but exhaustive: one I2C failure must not
        // skip the remaining rail shutdown or leave fast GPIOs driving the
        // unpowered panel.
        let mut first_error = None;
        if let Err(error) = self.digital_write_internal(IO_INT_ADDR, VCOM, false).await {
            first_error = Some(error);
        }
        if let Err(error) = self.digital_write_internal(IO_INT_ADDR, OE, false).await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        if let Err(error) = self.digital_write_internal(IO_INT_ADDR, GMOD, false).await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }

        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(false);
        if let Err(error) = self.digital_write_internal(IO_INT_ADDR, SPV, false).await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        if let Err(error) = self.digital_write_internal(IO_INT_ADDR, PWRUP, false).await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }

        let mut powered_down = false;
        let mut last_pg = PWR_GOOD_OK;
        for _ in 0..250 {
            embassy_time::Timer::after_millis(1).await;
            match self.read_power_good().await {
                Ok(0) => {
                    powered_down = true;
                    last_pg = 0;
                    break;
                }
                Ok(value) => last_pg = value,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    break;
                }
            }
        }
        if !powered_down && first_error.is_none() {
            first_error = Some(InkplateHalError::PanelPowerDownTimeout(last_pg));
        }

        if let Err(error) = self
            .digital_write_internal(IO_INT_ADDR, WAKEUP, false)
            .await
        {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        if let Err(error) = self.i2c_write(TPS65186_ADDR, &[0x01, 0x00]).await {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }

        // Match the reference driver's pinsZstate(): once the PMIC rails are
        // down, no scan or control pin may continue driving the unpowered
        // panel. prepare_panel_fast_io() restores all output directions on the
        // next transaction.
        GpioFast::out_enable_clear(PANEL_OUT_ENABLE_MASK);
        GpioFast::out_enable1_clear(PANEL_OUT1_ENABLE_MASK);
        self.panel_fast_ready = false;
        for pin in [OE, GMOD, SPV] {
            if let Err(error) = self
                .pin_mode_internal(IO_INT_ADDR, pin, PinMode::Input)
                .await
            {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        match first_error {
            Some(error) => {
                // Hardware state is uncertain. Keep shutdown-required state so
                // a later transaction cannot mistake this for a powered-off
                // panel and will retry recovery first.
                self.panel_power_state.shutdown_finished(false);
                Err(error)
            }
            None => {
                self.panel_power_state.shutdown_finished(true);
                Ok(())
            }
        }
    }
}
