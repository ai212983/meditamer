//! Shared PCAL6416A I/O expander register cache (typed observation
//! subscriptions plan, Phase 5).
//!
//! Was a private `InkplateHal` field (`io_regs_int`) until the battery
//! provider needed to toggle one of its pins (`FG_GPOUT`, waking the
//! BQ27441) from a task that owns no `InkplateHal` instance. The plan's own
//! diagnosis: "separate I2C handles alone cannot protect shared register
//! state" -- a read-modify-write of a cached register byte spans an
//! `await` (the I2C write that follows it), so two independent owners
//! could interleave mid-sequence and corrupt each other's change even with
//! the underlying I2C bus itself serialized. This type is now the single
//! owner of that cache; `InkplateHal` and the battery provider both reach
//! it through a shared `Mutex` (`InkplateHal::expander`), and every
//! register modify+write pair happens inside one lock acquisition, so a
//! caller never observes -- or races -- a partially-applied change.
//!
//! Failed or cancelled writes invalidate the cache before they can reach
//! the bus. The next mutator reconciles owned registers from the device
//! under that same lock; a partial pin-mode change cannot leak stale bits
//! into a later panel/frontlight/fuel-gauge update.

use super::{adapters::I2cOps, InkplateHalError, Result};

pub(super) const IO_INT_ADDR: u8 = 0x20;
pub(super) const FG_GPOUT: u8 = 15;
pub(super) const PCAL_REG_ADDRS: [u8; 23] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4F,
];
pub(super) const PCAL_OUTPORT0_ARRAY: usize = 2;
pub(super) const PCAL_OUTPORT1_ARRAY: usize = 3;
pub(super) const PCAL_CFGPORT0_ARRAY: usize = 6;
pub(super) const PCAL_CFGPORT1_ARRAY: usize = 7;
pub(super) const PCAL_PUPDEN_REG0_ARRAY: usize = 14;
pub(super) const PCAL_PUPDEN_REG1_ARRAY: usize = 15;
pub(super) const PCAL_PUPDSEL_REG0_ARRAY: usize = 16;
pub(super) const PCAL_PUPDSEL_REG1_ARRAY: usize = 17;

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) enum PinMode {
    Input,
    Output,
    InputPullUp,
    InputPullDown,
}

// Only these registers participate in cached read-modify-write. Reading
// input ports here would also acknowledge interrupts unrelated to recovery.
const CACHED_REGISTERS: [usize; 8] = [
    PCAL_OUTPORT0_ARRAY,
    PCAL_OUTPORT1_ARRAY,
    PCAL_CFGPORT0_ARRAY,
    PCAL_CFGPORT1_ARRAY,
    PCAL_PUPDEN_REG0_ARRAY,
    PCAL_PUPDEN_REG1_ARRAY,
    PCAL_PUPDSEL_REG0_ARRAY,
    PCAL_PUPDSEL_REG1_ARRAY,
];

/// The PCAL6416A at `IO_INT_ADDR`: its 23-byte cached register window plus
/// the I2C handle used to keep it in sync. Every caller reaches this
/// through a shared `Mutex`, never directly.
pub struct PcalExpander<I2C> {
    i2c: I2C,
    regs: [u8; 23],
    cache_valid: bool,
}

impl<I2C> PcalExpander<I2C>
where
    I2C: I2cOps,
{
    pub const fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            regs: [0; 23],
            cache_valid: false,
        }
    }

    /// Refreshes the cached output, configuration and pull registers.
    /// Failed reads leave the cache invalid and prevent later writes until
    /// reconciliation succeeds. Each register is addressed explicitly.
    pub async fn begin(&mut self) -> Result<(), I2C::Error> {
        self.cache_valid = false;
        self.ensure_cache().await
    }

    async fn ensure_cache(&mut self) -> Result<(), I2C::Error> {
        if !self.cache_valid {
            // Partial reads remain unusable while cache_valid is false. Fill
            // the owner cache in place instead of keeping another register
            // array alive across every await in the caller's task future.
            for idx in CACHED_REGISTERS {
                self.regs[idx] = self.read_i2c_reg(PCAL_REG_ADDRS[idx]).await?;
            }
            self.cache_valid = true;
        }
        Ok(())
    }

    /// `pub(crate)`: only `InkplateHal`'s own `i2c.rs` wrapper calls this
    /// directly. Other crates reach the one operation they need,
    /// `wake_fuel_gauge`, below.
    pub(crate) async fn pin_mode(&mut self, pin: u8, mode: PinMode) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }

        self.ensure_cache().await?;
        // Also covers a dropped future after a possibly applied write.
        self.cache_valid = false;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let cfg_idx = if port == 0 {
            PCAL_CFGPORT0_ARRAY
        } else {
            PCAL_CFGPORT1_ARRAY
        };
        let out_idx = if port == 0 {
            PCAL_OUTPORT0_ARRAY
        } else {
            PCAL_OUTPORT1_ARRAY
        };
        let pupden_idx = if port == 0 {
            PCAL_PUPDEN_REG0_ARRAY
        } else {
            PCAL_PUPDEN_REG1_ARRAY
        };
        let pupdsel_idx = if port == 0 {
            PCAL_PUPDSEL_REG0_ARRAY
        } else {
            PCAL_PUPDSEL_REG1_ARRAY
        };

        match mode {
            PinMode::Input => {
                self.modify_reg(cfg_idx, 1u8 << bit, 0);
                self.write_reg(cfg_idx).await?;
            }
            PinMode::Output => {
                self.modify_reg(cfg_idx, 0, 1u8 << bit);
                self.modify_reg(out_idx, 0, 1u8 << bit);
                self.write_reg(out_idx).await?;
                self.write_reg(cfg_idx).await?;
            }
            PinMode::InputPullUp => {
                self.modify_reg(cfg_idx, 1u8 << bit, 0);
                self.modify_reg(pupden_idx, 1u8 << bit, 0);
                self.modify_reg(pupdsel_idx, 1u8 << bit, 0);
                self.write_reg(cfg_idx).await?;
                self.write_reg(pupden_idx).await?;
                self.write_reg(pupdsel_idx).await?;
            }
            PinMode::InputPullDown => {
                self.modify_reg(cfg_idx, 1u8 << bit, 0);
                self.modify_reg(pupden_idx, 1u8 << bit, 0);
                self.modify_reg(pupdsel_idx, 0, 1u8 << bit);
                self.write_reg(cfg_idx).await?;
                self.write_reg(pupden_idx).await?;
                self.write_reg(pupdsel_idx).await?;
            }
        }
        self.cache_valid = true;
        Ok(())
    }

    /// Wakes the BQ27441 fuel gauge via a GPOUT pull-up edge -- the "narrow
    /// FG_GPOUT wake operation" the plan asks be exposed to the battery
    /// driver. Unchanged from `InkplateHal`'s former `wake_fuel_gauge`,
    /// just reached through this shared lock instead of a private field so
    /// a task with no `InkplateHal` instance can call it too.
    pub async fn wake_fuel_gauge(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode(FG_GPOUT, PinMode::InputPullUp).await?;
        embassy_time::Timer::after_millis(1).await;
        Ok(())
    }

    /// Drives active-low BUZZ_EN through the single register-cache owner.
    /// Write the desired output latch before enabling its output direction:
    /// preparing a disabled buzzer must not briefly pull its rail on.
    pub async fn set_buzzer_power(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        const BUZZ_EN: u8 = 12;
        self.digital_write(BUZZ_EN, !enabled).await?;
        if self.regs[PCAL_CFGPORT1_ARRAY] & (1 << (BUZZ_EN % 8)) != 0 {
            self.cache_valid = false;
            self.modify_reg(PCAL_CFGPORT1_ARRAY, 0, 1 << (BUZZ_EN % 8));
            self.write_reg(PCAL_CFGPORT1_ARRAY).await?;
            self.cache_valid = true;
        }
        Ok(())
    }

    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    pub(crate) async fn digital_write(&mut self, pin: u8, state: bool) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }

        self.ensure_cache().await?;
        self.cache_valid = false;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let out_idx = if port == 0 {
            PCAL_OUTPORT0_ARRAY
        } else {
            PCAL_OUTPORT1_ARRAY
        };
        if state {
            self.modify_reg(out_idx, 1u8 << bit, 0);
        } else {
            self.modify_reg(out_idx, 0, 1u8 << bit);
        }
        self.write_reg(out_idx).await?;
        self.cache_valid = true;
        Ok(())
    }

    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    pub(crate) async fn digital_read(&mut self, pin: u8) -> Result<bool, I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let in_idx = if port == 0 { 0 } else { 1 };
        let value = self.read_i2c_reg(PCAL_REG_ADDRS[in_idx]).await?;
        Ok((value & (1u8 << bit)) != 0)
    }

    fn modify_reg(&mut self, idx: usize, set_mask: u8, clear_mask: u8) {
        self.regs[idx] |= set_mask;
        self.regs[idx] &= !clear_mask;
    }

    async fn write_reg(&mut self, idx: usize) -> Result<(), I2C::Error> {
        let reg_value = self.regs[idx];
        self.i2c_write(IO_INT_ADDR, &[PCAL_REG_ADDRS[idx], reg_value])
            .await
    }

    async fn read_i2c_reg(&mut self, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf = [0u8; 1];
        self.i2c_write_read(IO_INT_ADDR, &[reg], &mut buf).await?;
        Ok(buf[0])
    }

    async fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
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

    async fn i2c_write_read(
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
}

#[cfg(test)]
#[path = "expander_tests.rs"]
mod tests;
