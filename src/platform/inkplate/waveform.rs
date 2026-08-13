use super::{
    DelayOps, GpioFast, I2cOps, InkplateHal, Result, CKV_MASK1, CL_MASK, DATA_MASK, IO_INT_ADDR,
    LE_MASK, SPH_MASK1, SPV,
};

// Direct consecutive GPIO writes make CL too narrow at the firmware's 240 MHz
// CPU clock. Hardware validation found that holding it high for 24 CPU cycles
// prevents horizontal-band corruption while preserving fast partial updates;
// shorter holds remain opt-in hardware experiments.
pub(crate) const PANEL_CL_HIGH_HOLD_CYCLES: u32 =
    if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_6").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_9").is_some() {
        9
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_12").is_some() {
        12
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_24").is_some() {
        24
    } else {
        48
    };

#[inline(always)]
fn hold_panel_clock_high() {
    esp_hal::xtensa_lx::timer::delay(PANEL_CL_HIGH_HOLD_CYCLES);
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub(super) async fn vscan_start(&mut self) -> Result<(), I2C::Error> {
        self.set_ckv(true);
        self.delay.delay_us(7);
        self.digital_write_internal(IO_INT_ADDR, SPV, false).await?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(8);
        self.digital_write_internal(IO_INT_ADDR, SPV, true).await?;
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

    #[inline(always)]
    pub(super) fn vscan_end(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
        // Soldered's reference driver calls the ESP ROM delay primitive here
        // with a nominal zero duration. That call-latency-only boundary proved
        // code-layout-sensitive in the Rust release build, so retain an
        // explicit 1 us setup margin before the next SPH/CKV sequence.
        esp_hal::rom::ets_delay_us(1);
    }

    #[inline(always)]
    pub(super) fn hscan_start(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_preserve_data(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn pulse_cl_only(&self) {
        GpioFast::out_set(CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn clear_data_and_cl_le(&self) {
        GpioFast::out_clear(DATA_MASK | LE_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn set_le(&self, high: bool) {
        if high {
            GpioFast::out_set(LE_MASK);
        } else {
            GpioFast::out_clear(LE_MASK);
        }
    }

    #[inline(always)]
    pub(super) fn set_cl(&self, high: bool) {
        if high {
            GpioFast::out_set(CL_MASK);
        } else {
            GpioFast::out_clear(CL_MASK);
        }
    }

    #[inline(always)]
    pub(super) fn set_ckv(&self, high: bool) {
        if high {
            GpioFast::out1_set(CKV_MASK1);
        } else {
            GpioFast::out1_clear(CKV_MASK1);
        }
    }

    #[inline(always)]
    pub(super) fn set_sph(&self, high: bool) {
        if high {
            GpioFast::out1_set(SPH_MASK1);
        } else {
            GpioFast::out1_clear(SPH_MASK1);
        }
    }
}
