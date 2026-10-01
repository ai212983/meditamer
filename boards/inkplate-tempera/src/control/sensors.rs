use super::super::{
    DebugSnapshot, DelayOps, I2cOps, InkplateHal, PinMode, Result, BATTERY_MEAS_EN, IO_INT_ADDR,
};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    // `wake_fuel_gauge` moved to `expander::PcalExpander` (typed observation
    // subscriptions plan, Phase 5): the battery provider task calls it
    // directly through the shared `Mutex`, the same way `InkplateHal` itself
    // now reaches every other PCAL operation -- see that type's module doc.
    // Routing it through `InkplateHal`/`display_task` instead would have
    // made the battery provider's acquisition depend on the display task's
    // event-loop cadence being free to service a request, which is
    // exactly what is *not* true while `display_task` is itself blocked
    // inside `panel_bus::suspend_clients` awaiting that same provider's
    // suspend acknowledgement -- a real deadlock, not a hypothetical one.

    pub async fn battery_measurement_enable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity().await?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, gate_active_high)
            .await?;
        embassy_time::Timer::after_millis(5).await;
        Ok(())
    }

    pub async fn battery_measurement_disable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity().await?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)
            .await
    }

    async fn detect_battery_gate_polarity(&mut self) -> Result<bool, I2C::Error> {
        if let Some(gate_active_high) = self.battery_gate_active_high {
            return Ok(gate_active_high);
        }

        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Input)
            .await?;
        let idle_state_high = self
            .digital_read_internal(IO_INT_ADDR, BATTERY_MEAS_EN)
            .await?;
        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Output)
            .await?;

        // Arduino reference uses the level observed while floating to detect board revision.
        // If pin reads low, gate is enabled by driving high on newer revisions.
        let gate_active_high = !idle_state_high;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)
            .await?;
        self.battery_gate_active_high = Some(gate_active_high);
        Ok(gate_active_high)
    }

    pub async fn debug_snapshot(&mut self) -> Result<DebugSnapshot, I2C::Error> {
        Ok(DebugSnapshot {
            pcal_out0: self.read_i2c_reg(IO_INT_ADDR, 0x02).await?,
            pcal_out1: self.read_i2c_reg(IO_INT_ADDR, 0x03).await?,
            pcal_cfg0: self.read_i2c_reg(IO_INT_ADDR, 0x06).await?,
            pcal_cfg1: self.read_i2c_reg(IO_INT_ADDR, 0x07).await?,
        })
    }
}
