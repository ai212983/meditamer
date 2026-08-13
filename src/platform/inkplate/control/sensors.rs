use super::super::{
    DebugSnapshot, DelayOps, I2cOps, InkplateHal, PinMode, Result, BATTERY_MEAS_EN,
    BQ27441_COMMAND_SOC, FG_GPOUT, FUEL_GAUGE_ADDR, IO_INT_ADDR,
};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub async fn fuel_gauge_soc(&mut self) -> Result<u16, I2C::Error> {
        self.wake_fuel_gauge().await?;
        self.read_i2c_reg_u16_le(FUEL_GAUGE_ADDR, BQ27441_COMMAND_SOC)
            .await
    }

    pub async fn wake_fuel_gauge(&mut self) -> Result<(), I2C::Error> {
        // Inkplate 4 TEMPERA reference wakes BQ27441 via GPOUT pull-up edge.
        self.pin_mode_internal(IO_INT_ADDR, FG_GPOUT, PinMode::InputPullUp)
            .await?;
        embassy_time::Timer::after_millis(1).await;
        Ok(())
    }

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
