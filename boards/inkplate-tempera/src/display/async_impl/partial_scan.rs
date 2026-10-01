use super::super::super::{
    clean::with_scan_interrupts_masked, DelayOps, GpioFast, I2cOps, InkplateHal,
    PartialGateDrainTiming, Result, CL_MASK, DATA_MASK, E_INK_HEIGHT, E_INK_WIDTH,
    PANEL_PARTIAL_HARDWARE_LOOP, PANEL_PARTIAL_HOISTED_GPIO_LOOP, PANEL_PARTIAL_SCAN_PHASE_TIMING,
    PARTIAL_TRANSITION_BYTES,
};
use crate::partial_transition::PartialRowSpan;
use embassy_time::Instant;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub(super) async fn display_bw_partial_waveform_async(&mut self) -> Result<(), I2C::Error> {
        for _ in 0..9 {
            self.vscan_start().await?;
            self.scan_partial_framebuffer_pass();
            self.delay.delay_us(230);
        }

        self.clean_async(2, 2).await?;
        self.clean_async(3, 1).await?;
        self.vscan_start().await?;
        Ok(())
    }

    pub(super) async fn display_bw_partial_gate_drain_waveform_timed_async(
        &mut self,
        row_span: PartialRowSpan,
    ) -> Result<PartialGateDrainTiming, I2C::Error> {
        let scan_rows = row_span.scan_rows;
        debug_assert!((1..=E_INK_HEIGHT).contains(&scan_rows));
        let transition_started_us = Instant::now().as_micros();
        let mut vscan_start_us = 0u64;
        let mut source_scan_us = 0u64;
        let mut pass_delay_us = 0u64;
        for _ in 0..9 {
            let phase_started_us = if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                Instant::now().as_micros()
            } else {
                0
            };
            self.vscan_start().await?;
            if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                vscan_start_us = vscan_start_us
                    .saturating_add(Instant::now().as_micros().saturating_sub(phase_started_us));
            }
            let transition = self
                .partial_transition
                .as_deref()
                .expect("partial transition buffer checked before panel power-on");
            let phase_started_us = if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                Instant::now().as_micros()
            } else {
                0
            };
            self.scan_partial_framebuffer_rows_with_neutral_drain(transition, scan_rows);
            if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                source_scan_us = source_scan_us
                    .saturating_add(Instant::now().as_micros().saturating_sub(phase_started_us));
            }
            let phase_started_us = if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                Instant::now().as_micros()
            } else {
                0
            };
            self.delay.delay_us(230);
            if PANEL_PARTIAL_SCAN_PHASE_TIMING {
                pass_delay_us = pass_delay_us
                    .saturating_add(Instant::now().as_micros().saturating_sub(phase_started_us));
            }
        }
        let transition_us = Instant::now()
            .as_micros()
            .saturating_sub(transition_started_us);

        let cleanup_neutral_started_us = Instant::now().as_micros();
        self.vscan_start().await?;
        let cleanup_neutral_finalize_us = Instant::now()
            .as_micros()
            .saturating_sub(cleanup_neutral_started_us);

        Ok(PartialGateDrainTiming {
            scan_rows,
            first_changed_row: row_span.first_changed_row,
            last_changed_row: row_span.last_changed_row,
            changed_span_rows: row_span.changed_span_rows,
            source_skip_candidate_rows: row_span.source_skip_candidate_rows,
            row_discovery_us: 0,
            transition_prepare_us: 0,
            power_on_us: 0,
            transition_us,
            vscan_start_us,
            source_scan_us,
            pass_delay_us,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us,
            finalization_us: 0,
            previous_copy_us: 0,
            full_fallback: false,
        })
    }

    /// Scans one complete partial waveform frame without yielding while panel
    /// row timing is live. The reference drivers only reschedule between
    /// complete 600-row passes; yielding mid-pass collapses horizontal bands.
    #[esp_hal::ram]
    fn scan_partial_framebuffer_pass(&self) {
        with_scan_interrupts_masked(|| {
            let transition = self
                .partial_transition
                .as_deref()
                .expect("partial transition buffer checked before panel power-on");
            let mut position = PARTIAL_TRANSITION_BYTES as isize - 1;
            for _ in 0..E_INK_HEIGHT {
                let data = transition[position as usize];
                let send = self.pin_lut[data as usize];
                self.hscan_start(send);
                position -= 1;

                for _ in 0..(E_INK_WIDTH / 4 - 1) {
                    let data = transition[position as usize];
                    let send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                    position -= 1;
                }

                self.pulse_cl_only();
                self.vscan_end();
            }
            debug_assert_eq!(position, -1);
        });
    }

    #[esp_hal::ram]
    fn scan_partial_framebuffer_rows_with_neutral_drain(
        &self,
        transition: &[u8; PARTIAL_TRANSITION_BYTES],
        scan_rows: usize,
    ) {
        with_scan_interrupts_masked(|| {
            let mut position = PARTIAL_TRANSITION_BYTES;
            let out_set = GpioFast::out_set_ptr();
            let out_clear = GpioFast::out_clear_ptr();
            let clear_mask = DATA_MASK | CL_MASK;
            for _ in 0..scan_rows {
                position -= 1;
                // SAFETY: transition has the exact full-frame size and the caller
                // constrains scan_rows to 1..=E_INK_HEIGHT, so the loop consumes at
                // most PARTIAL_TRANSITION_BYTES entries.
                let data = unsafe { *transition.get_unchecked(position) };
                let send = self.pin_lut[data as usize];
                self.hscan_start(send);

                #[cfg(target_arch = "xtensa")]
                if PANEL_PARTIAL_HOISTED_GPIO_LOOP {
                    // SAFETY: `position` points one byte past this row's 149
                    // remaining source words, while the GPIO pointers are the
                    // live W1TS/W1TC registers. The assembly only advances the
                    // local source pointer and writes those registers.
                    unsafe {
                        scan_partial_source_words_hoisted(
                            transition.as_ptr().add(position),
                            self.pin_lut.as_ptr(),
                            out_set,
                            out_clear,
                            clear_mask,
                        );
                    }
                    position -= E_INK_WIDTH / 4 - 1;
                } else {
                    for _ in 0..(E_INK_WIDTH / 4 - 1) {
                        position -= 1;
                        // SAFETY: same fixed-size and scan_rows invariant as above.
                        let data = unsafe { *transition.get_unchecked(position) };
                        let send = self.pin_lut[data as usize];
                        self.write_data_and_clock(send);
                    }
                }

                #[cfg(not(target_arch = "xtensa"))]
                for _ in 0..(E_INK_WIDTH / 4 - 1) {
                    position -= 1;
                    // SAFETY: same fixed-size and scan_rows invariant as above.
                    let data = unsafe { *transition.get_unchecked(position) };
                    let send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                }

                self.pulse_cl_only();
                self.vscan_end();
            }
            let drain_rows = E_INK_HEIGHT - scan_rows;
            if drain_rows != 0 {
                // 0xFF is the partial waveform's no-transition command for four
                // pixels. Shift it once so every omitted gate sees a neutral source
                // row, then keep advancing CKV to the normal 600-row terminal
                // state. The CKV-only advance intentionally has no added hold.
                let neutral = self.pin_lut[0xFF];
                self.hscan_start(neutral);
                for _ in 0..(E_INK_WIDTH / 4 - 1) {
                    self.write_data_and_clock(neutral);
                }
                self.pulse_cl_only();
                self.vscan_end();

                for _ in 1..drain_rows {
                    self.set_ckv(true);
                    esp_hal::rom::ets_delay_us(0);
                    self.vscan_end();
                }
            }
        });
    }
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
unsafe fn scan_partial_source_words_hoisted(
    source: *const u8,
    pin_lut: *const u32,
    out_set: *mut u32,
    out_clear: *mut u32,
    clear_mask: u32,
) {
    let remaining = E_INK_WIDTH / 4 - 1;
    let cl_mask = CL_MASK;

    // SAFETY: the caller supplies a source pointer one byte past 149 readable
    // transition words, a 256-entry LUT, and the live GPIO W1TS/W1TC pointers.
    // Keeping the complete fixed-count loop in one assembly block makes the
    // addresses loop invariants while retaining the default path's exact
    // memory barriers and GPIO write order.
    if PANEL_PARTIAL_HARDWARE_LOOP {
        unsafe {
            core::arch::asm!(
                "loop {remaining}, 2f",
                "addi {source}, {source}, -1",
                "l8ui {data}, {source}, 0",
                "addx4 {data}, {data}, {pin_lut}",
                "l32i {data}, {data}, 0",
                "or {data}, {data}, {cl_mask}",
                "memw",
                "s32i {data}, {out_set}, 0",
                "memw",
                "s32i {clear_mask}, {out_clear}, 0",
                "2:",
                source = inout(reg) source => _,
                data = out(reg) _,
                remaining = in(reg) remaining,
                pin_lut = in(reg) pin_lut,
                out_set = in(reg) out_set,
                out_clear = in(reg) out_clear,
                cl_mask = in(reg) cl_mask,
                clear_mask = in(reg) clear_mask,
                options(nostack)
            );
        }
    } else {
        unsafe {
            core::arch::asm!(
                "2:",
                "addi {source}, {source}, -1",
                "l8ui {data}, {source}, 0",
                "addx4 {data}, {data}, {pin_lut}",
                "l32i {data}, {data}, 0",
                "or {data}, {data}, {cl_mask}",
                "memw",
                "s32i {data}, {out_set}, 0",
                "memw",
                "s32i {clear_mask}, {out_clear}, 0",
                "addi {remaining}, {remaining}, -1",
                "bnez {remaining}, 2b",
                source = inout(reg) source => _,
                data = out(reg) _,
                remaining = inout(reg) remaining => _,
                pin_lut = in(reg) pin_lut,
                out_set = in(reg) out_set,
                out_clear = in(reg) out_clear,
                cl_mask = in(reg) cl_mask,
                clear_mask = in(reg) clear_mask,
                options(nostack)
            );
        }
    }
}
