use super::{
    DelayOps, FullRefreshTiming, GpioFast, I2cOps, InkplateHal, Result, CL_MASK, DATA_MASK,
    E_INK_HEIGHT, E_INK_WIDTH, PANEL_FULL_CLEAN_HARDWARE_LOOP, PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES,
    PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE, PANEL_FULL_FIXED_HOLD_SELECTED,
    PANEL_FULL_INTER_PASS_DELAY_US, PANEL_FULL_OPTIMIZED_CLEAN_LOOP, PANEL_FULL_REFERENCE_SEQUENCE,
};
use embassy_time::Instant;
use esp_sync::raw::{RawLock, SingleCoreInterruptLock};

#[cfg(target_arch = "xtensa")]
static SCAN_MASKED_MAX_US: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Maximum completed scan closure while local interrupts were masked.
/// Excludes lock entry/exit; timing uses the HAL microsecond clock.
pub fn scan_masked_max_us() -> u32 {
    #[cfg(target_arch = "xtensa")]
    return SCAN_MASKED_MAX_US.load(core::sync::atomic::Ordering::Relaxed);
    #[cfg(not(target_arch = "xtensa"))]
    0
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
fn record_scan_masked(elapsed: u32) {
    // Only the CPU0 panel owner writes; serial only reads this observation.
    // Avoid a compare/exchange helper inside the waveform function.
    use core::sync::atomic::Ordering;
    if elapsed > SCAN_MASKED_MAX_US.load(Ordering::Relaxed) {
        SCAN_MASKED_MAX_US.store(elapsed, Ordering::Relaxed);
    }
}

/// Runs one cache-disabled scan with only this core's interrupts masked.
/// The closure keeps nested locks LIFO and prevents the guard from escaping.
#[inline(always)]
#[unsafe(link_section = ".rwtext")]
pub(super) fn with_scan_interrupts_masked(run: impl FnOnce()) {
    let lock = SingleCoreInterruptLock;
    // SAFETY: this scope retains the restore state, cannot cross an await or
    // core boundary, and nested calls return before their outer scope exits.
    let state = unsafe { lock.enter() };
    #[cfg(target_arch = "xtensa")]
    let started = esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros() as u32;
    run();
    #[cfg(target_arch = "xtensa")]
    let elapsed = (esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros() as u32)
        .wrapping_sub(started);
    // SAFETY: paired with the entry above in the same non-yielding scope.
    unsafe { lock.exit(state) };
    #[cfg(target_arch = "xtensa")]
    record_scan_masked(elapsed);
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub(super) async fn clean_async(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start().await?;
            self.scan_clean_pass(send);
            self.delay.delay_us(230);
        }
        Ok(())
    }

    pub(super) async fn clean_full_refresh_async(
        &mut self,
        c: u8,
        rep: u8,
    ) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start().await?;
            self.scan_full_refresh_clean_pass(send);
            if PANEL_FULL_INTER_PASS_DELAY_US != 0 {
                self.delay.delay_us(PANEL_FULL_INTER_PASS_DELAY_US);
            }
        }
        Ok(())
    }

    /// Profile-only variant that attributes the repeated pass setup, source
    /// scan, and optional spacing without adding clock reads to production.
    pub(super) async fn clean_full_refresh_profiled_async(
        &mut self,
        c: u8,
        rep: u8,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            let started_us = Instant::now().as_micros();
            self.vscan_start().await?;
            timing.vscan_start_us = timing
                .vscan_start_us
                .saturating_add(Instant::now().as_micros().saturating_sub(started_us));

            let started_us = Instant::now().as_micros();
            self.scan_full_refresh_clean_pass(send);
            timing.source_scan_us = timing
                .source_scan_us
                .saturating_add(Instant::now().as_micros().saturating_sub(started_us));

            if PANEL_FULL_INTER_PASS_DELAY_US != 0 {
                let started_us = Instant::now().as_micros();
                self.delay.delay_us(PANEL_FULL_INTER_PASS_DELAY_US);
                timing.pass_delay_us = timing
                    .pass_delay_us
                    .saturating_add(Instant::now().as_micros().saturating_sub(started_us));
            }
        }
        Ok(())
    }

    pub(super) async fn clean_grayscale_async(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start().await?;
            self.scan_grayscale_clean_pass(send);
            self.delay.delay_us(230);
        }
        Ok(())
    }

    /// A complete cleanup frame is one timing transaction. It runs from IRAM
    /// with interrupts masked so unrelated firmware activity cannot stretch a
    /// CKV/LE row boundary and produce horizontal bands.
    #[esp_hal::ram]
    fn scan_clean_pass(&self, send: u32) {
        with_scan_interrupts_masked(|| {
            for _ in 0..E_INK_HEIGHT {
                self.hscan_start(send);
                self.write_data_and_clock_preserve_data(send);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }
                self.write_data_and_clock(send);
                self.vscan_end();
            }
        });
    }

    /// The full-refresh cleanup sequence uses its independently qualified
    /// constant-data pulse timing while retaining the reference scan ordering.
    #[esp_hal::ram]
    fn scan_full_refresh_clean_pass(&self, send: u32) {
        with_scan_interrupts_masked(|| {
            let out_set = GpioFast::out_set_ptr();
            let out_clear = GpioFast::out_clear_ptr();
            for _ in 0..E_INK_HEIGHT {
                if PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE {
                    self.set_sph(false);
                    GpioFast::out_set(send | CL_MASK);
                    GpioFast::out_clear(DATA_MASK | CL_MASK);
                    self.set_sph(true);
                    self.set_ckv(true);
                    GpioFast::out_set(send | CL_MASK);
                    GpioFast::out_clear(CL_MASK);
                } else {
                    self.hscan_start_full_refresh(send);
                    self.write_data_and_clock_preserve_data_full_refresh(send);
                }
                #[cfg(target_arch = "xtensa")]
                if PANEL_FULL_OPTIMIZED_CLEAN_LOOP {
                    // SAFETY: the pointers come from the live GPIO block and
                    // the helper emits the fixed 148 CL-only pulses for this
                    // row without touching memory outside those registers.
                    unsafe { scan_full_clean_cl_pulses_hoisted(out_set, out_clear) };
                } else {
                    for _ in 0..(E_INK_WIDTH / 8 - 1) {
                        self.pulse_cl_only_full_refresh();
                        self.pulse_cl_only_full_refresh();
                    }
                }
                #[cfg(not(target_arch = "xtensa"))]
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only_full_refresh();
                    self.pulse_cl_only_full_refresh();
                }
                if PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE {
                    GpioFast::out_set(send | CL_MASK);
                    GpioFast::out_clear(DATA_MASK | CL_MASK);
                } else {
                    self.write_data_and_clock_full_refresh(send);
                }
                self.vscan_end_full_refresh();
            }
        });
    }

    /// The native grayscale cleanup sequence must not inherit the partial
    /// refresh hold: its timing is qualified with the Gray4 framebuffer scan.
    #[esp_hal::ram]
    fn scan_grayscale_clean_pass(&self, send: u32) {
        with_scan_interrupts_masked(|| {
            for _ in 0..E_INK_HEIGHT {
                self.hscan_start_grayscale(send);
                self.write_data_and_clock_preserve_data_grayscale(send);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only_grayscale();
                    self.pulse_cl_only_grayscale();
                }
                self.write_data_and_clock_grayscale(send);
                self.vscan_end_grayscale();
            }
        });
    }
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
unsafe fn scan_full_clean_cl_pulses_hoisted(out_set: *mut u32, out_clear: *mut u32) {
    const PULSE_PAIRS_PER_ROW: usize = E_INK_WIDTH / 8 - 1;
    const PULSES_PER_ROW: usize = PULSE_PAIRS_PER_ROW * 2;
    const _: () = assert!(
        !PANEL_FULL_OPTIMIZED_CLEAN_LOOP
            || (!PANEL_FULL_FIXED_HOLD_SELECTED && !PANEL_FULL_REFERENCE_SEQUENCE)
    );
    let remaining = PULSES_PER_ROW;
    // SAFETY: the caller supplies the live bank-0 W1TS/W1TC addresses. This
    // block preserves ordered memw/set/(optional CCOUNT wait)/memw/clear while
    // making both addresses and the loop trip count explicit invariants.
    if PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES == 0 && PANEL_FULL_CLEAN_HARDWARE_LOOP {
        let remaining = PULSE_PAIRS_PER_ROW;
        // SAFETY: this is the reference assembly's fixed 74-iteration shape:
        // each hardware-loop iteration emits exactly two ordered CL pulses.
        unsafe {
            core::arch::asm!(
                "loop {remaining}, 2f",
                "memw",
                "s32i {cl_mask}, {out_set}, 0",
                "memw",
                "s32i {cl_mask}, {out_clear}, 0",
                "memw",
                "s32i {cl_mask}, {out_set}, 0",
                "memw",
                "s32i {cl_mask}, {out_clear}, 0",
                "2:",
                remaining = inout(reg) remaining => _,
                out_set = in(reg) out_set,
                out_clear = in(reg) out_clear,
                cl_mask = in(reg) CL_MASK,
                options(nostack)
            );
        }
    } else if PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES == 0 {
        unsafe {
            core::arch::asm!(
                "2:",
                "memw",
                "s32i {cl_mask}, {out_set}, 0",
                "memw",
                "s32i {cl_mask}, {out_clear}, 0",
                "addi {remaining}, {remaining}, -1",
                "bnez {remaining}, 2b",
                remaining = inout(reg) remaining => _,
                out_set = in(reg) out_set,
                out_clear = in(reg) out_clear,
                cl_mask = in(reg) CL_MASK,
                options(nostack)
            );
        }
    } else {
        unsafe {
            core::arch::asm!(
                "2:",
                "memw",
                "s32i {cl_mask}, {out_set}, 0",
                "rsr.ccount {started}",
                "3:",
                "rsr.ccount {elapsed}",
                "sub {elapsed}, {elapsed}, {started}",
                "bltui {elapsed}, {hold_cycles}, 3b",
                "memw",
                "s32i {cl_mask}, {out_clear}, 0",
                "addi {remaining}, {remaining}, -1",
                "bnez {remaining}, 2b",
                remaining = inout(reg) remaining => _,
                started = out(reg) _,
                elapsed = out(reg) _,
                out_set = in(reg) out_set,
                out_clear = in(reg) out_clear,
                cl_mask = in(reg) CL_MASK,
                hold_cycles = const PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES,
                options(nostack)
            );
        }
    }
}
