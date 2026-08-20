//! SD card power rail control, trimmed from [`crate::platform::inkplate`]'s
//! full `InkplateHal` driver to the one PCAL9535A IO-expander pin the
//! updater touches. The full driver is not reused here on purpose: it
//! allocates the e-ink panel's static framebuffer and waveform tables on
//! construction, which would defeat the point of measuring a *minimal*
//! updater (ADR-0014 Phase 1). Register semantics mirror
//! `src/platform/inkplate/{i2c,base,hardware}.rs::{sd_card_power_on,
//! digital_write_internal, pin_mode_internal}` exactly — same chip address,
//! same registers, same bit, same write order.

use embedded_hal_async::i2c::I2c;

const IO_INT_ADDR: u8 = 0x20;
/// PCAL9535A "IO_INT" output-port-1 register. `PCAL_OUTPORT1_ARRAY` in
/// `src/platform/inkplate/hardware.rs` is `3`, an index into
/// `PCAL_REG_ADDRS`, not a register address itself —
/// `PCAL_REG_ADDRS[3] == 0x03` (the "legacy" 0x00-0x07 bank), not `0x43`
/// (the "enhanced" 0x40+ bank, a different set of registers — drive
/// strength/latch/etc. — that don't affect direction or output level).
const OUTPORT1_REG: u8 = 0x03;
/// PCAL9535A "IO_INT" config-port-1 register: `PCAL_REG_ADDRS[7] == 0x07`,
/// same index-vs-address distinction as `OUTPORT1_REG` above.
const CFGPORT1_REG: u8 = 0x07;
/// `SD_PMOS_PIN` (pin 11) falls in port 1 (`pin / 8`), bit 3 (`pin % 8`).
const SD_PMOS_BIT: u8 = 3;

async fn read_reg<I2C: I2c>(i2c: &mut I2C, reg: u8) -> Result<u8, I2C::Error> {
    let mut value = [0u8; 1];
    i2c.write_read(IO_INT_ADDR, &[reg], &mut value).await?;
    Ok(value[0])
}

async fn write_reg<I2C: I2c>(i2c: &mut I2C, reg: u8, value: u8) -> Result<(), I2C::Error> {
    i2c.write(IO_INT_ADDR, &[reg, value]).await
}

/// Drives `SD_PMOS_PIN` low through an output-mode config, powering the SD
/// rail. Preserves every other bit in both registers (other pins on the same
/// port) via read-modify-write, since this device is never the only owner of
/// the IO expander across the board's lifetime.
pub(super) async fn power_on<I2C: I2c>(i2c: &mut I2C) -> Result<(), I2C::Error> {
    // Order matches `pin_mode_internal(Output)`: write the output register
    // (already driven low) before switching the pin to output, then
    // `digital_write_internal(false)` writes the (already-low) output
    // register once more. Cheap to repeat exactly rather than "simplify" a
    // sequence proven on hardware we cannot re-verify here.
    let outport1 = read_reg(i2c, OUTPORT1_REG).await? & !(1 << SD_PMOS_BIT);
    write_reg(i2c, OUTPORT1_REG, outport1).await?;
    let cfgport1 = read_reg(i2c, CFGPORT1_REG).await? & !(1 << SD_PMOS_BIT);
    write_reg(i2c, CFGPORT1_REG, cfgport1).await?;
    write_reg(i2c, OUTPORT1_REG, outport1).await?;
    Ok(())
}
