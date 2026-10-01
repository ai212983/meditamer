use super::{
    buzzer::PitchWriteDelay, DelayOps, I2cOps, InkplateHal, InkplateHalError, PinMode, Result,
    BUZZER_DIGIPOT_ADDR, ENABLE_BUZZER_PITCH_CONTROL, IO_INT_ADDR,
};

/// Millisecond waits for retried pitch writes on device.
struct EmbassyDelay;

impl PitchWriteDelay for EmbassyDelay {
    async fn wait_ms(&mut self, ms: u64) {
        embassy_time::Timer::after_millis(ms).await;
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    /// Delegates to the shared `PcalExpander` under its `Mutex` -- see that
    /// type's module doc for why the register cache these three (plus
    /// `pin_mode_internal`) drive lives there now, not in a private field
    /// here. `addr` is always `IO_INT_ADDR`; every call site already only
    /// ever passes that, matching the single expander this board wires.
    pub(super) async fn pin_mode_internal(
        &mut self,
        addr: u8,
        pin: u8,
        mode: PinMode,
    ) -> Result<(), I2C::Error> {
        debug_assert_eq!(
            addr, IO_INT_ADDR,
            "only one expander is wired on this board"
        );
        self.expander.lock().await.pin_mode(pin, mode).await
    }

    pub(super) async fn digital_write_internal(
        &mut self,
        addr: u8,
        pin: u8,
        state: bool,
    ) -> Result<(), I2C::Error> {
        debug_assert_eq!(
            addr, IO_INT_ADDR,
            "only one expander is wired on this board"
        );
        self.expander.lock().await.digital_write(pin, state).await
    }

    pub(super) async fn digital_read_internal(
        &mut self,
        addr: u8,
        pin: u8,
    ) -> Result<bool, I2C::Error> {
        debug_assert_eq!(
            addr, IO_INT_ADDR,
            "only one expander is wired on this board"
        );
        self.expander.lock().await.digital_read(pin).await
    }

    pub(super) async fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        match self.i2c.write(addr, bytes).await {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset().await;
                embassy_time::Timer::after_millis(1).await;
                self.i2c
                    .write(addr, bytes)
                    .await
                    .map_err(InkplateHalError::I2c)
            }
        }
    }

    pub(super) async fn i2c_write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2C::Error> {
        match self.i2c.write_read(addr, bytes, buffer).await {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset().await;
                embassy_time::Timer::after_millis(1).await;
                self.i2c
                    .write_read(addr, bytes, buffer)
                    .await
                    .map_err(InkplateHalError::I2c)
            }
        }
    }

    pub(super) async fn read_i2c_reg(&mut self, addr: u8, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf = [0u8; 1];
        self.i2c_write_read(addr, &[reg], &mut buf).await?;
        Ok(buf[0])
    }

    pub async fn i2c_fault_recovery_smoke(&mut self, attempts: u8) -> Result<(), I2C::Error> {
        let tries = attempts.max(1);
        for _ in 0..tries {
            // 0x7F is intentionally unused in this design; this forces an address NACK.
            let _ = self.i2c_write(0x7F, &[0x00]).await;
            embassy_time::Timer::after_millis(1).await;
        }

        let _ = self.read_i2c_reg(IO_INT_ADDR, 0x00).await?;
        Ok(())
    }

    /// Programs one rheostat code on the powered rail with bounded retries.
    ///
    /// Exhausted transport failures propagate to the caller; recovery touches
    /// only the buzzer path and never enables unrelated subsystems. Requires
    /// a powered rail: the rail powers the rheostat being programmed.
    pub(super) async fn write_buzzer_code(&mut self, code: u8) -> Result<(), I2C::Error> {
        if !ENABLE_BUZZER_PITCH_CONTROL {
            return Ok(());
        }
        super::buzzer::write_pitch_code(&mut self.i2c, BUZZER_DIGIPOT_ADDR, code, &mut EmbassyDelay)
            .await
            .map_err(InkplateHalError::I2c)
    }
}
