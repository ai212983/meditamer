use super::super::super::{
    clean::with_scan_interrupts_masked, DelayOps, GpioFast, I2cOps, InkplateHal, CL_MASK,
    DATA_MASK, E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES,
    GRAYSCALE_WAVEFORM_LUT, PANEL_FULL_CL_HIGH_HOLD_CYCLES, PANEL_FULL_FIXED_HOLD_SELECTED,
    PANEL_FULL_HARDWARE_LOOP, PANEL_FULL_PIPELINED_HOLD, PANEL_FULL_REFERENCE_SEQUENCE,
    PANEL_FULL_ROW_START_REFERENCE_SEQUENCE, PANEL_FULL_SOURCE_FIXED_HOLD_NOPS,
    PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED,
};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    #[esp_hal::ram]
    pub(super) fn scan_grayscale_framebuffer_pass(
        &self,
        framebuffer: &[u8; GRAYSCALE_FRAMEBUFFER_BYTES],
        phase: usize,
    ) {
        with_scan_interrupts_masked(|| {
            let phase_lut = &GRAYSCALE_WAVEFORM_LUT[phase];
            let mut position = GRAYSCALE_FRAMEBUFFER_BYTES as isize;
            for _ in 0..E_INK_HEIGHT {
                position -= 1;
                // SAFETY: the array type fixes the full-frame length, and the
                // 600-by-300-byte traversal consumes each byte once from the end
                // without crossing index zero.
                let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                let upper = phase_lut[pixel as usize] << 4;
                position -= 1;
                // SAFETY: same exact-length traversal invariant as above.
                let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                let lower = phase_lut[pixel as usize];
                let mut send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                self.hscan_start_grayscale(send);

                position -= 1;
                // SAFETY: same exact-length traversal invariant as above.
                let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                let upper = phase_lut[pixel as usize] << 4;
                position -= 1;
                // SAFETY: same exact-length traversal invariant as above.
                let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                let lower = phase_lut[pixel as usize];
                send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                self.write_data_and_clock_grayscale(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    position -= 1;
                    // SAFETY: same exact-length traversal invariant as above.
                    let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                    let upper = phase_lut[pixel as usize] << 4;
                    position -= 1;
                    // SAFETY: same exact-length traversal invariant as above.
                    let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                    let lower = phase_lut[pixel as usize];
                    send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                    self.write_data_and_clock_grayscale(send);

                    position -= 1;
                    // SAFETY: same exact-length traversal invariant as above.
                    let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                    let upper = phase_lut[pixel as usize] << 4;
                    position -= 1;
                    // SAFETY: same exact-length traversal invariant as above.
                    let pixel = unsafe { *framebuffer.get_unchecked(position as usize) };
                    let lower = phase_lut[pixel as usize];
                    send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                    self.write_data_and_clock_grayscale(send);
                }

                self.pulse_cl_only_grayscale();
                self.vscan_end_grayscale();
            }
            debug_assert_eq!(position, 0);
        });
    }

    #[esp_hal::ram]
    pub(super) fn scan_full_binary_pass(&self, waveform_lut: &'static [u8; 16]) {
        // The waveform is local to this core and must not use the ESP32
        // critical-section implementation: that implementation also takes a
        // cross-core spinlock. Holding it for an entire 600-row scan can stall
        // the other core for tens of milliseconds and disturb panel timing.
        with_scan_interrupts_masked(|| {
            let mut position = FRAMEBUFFER_BYTES as isize - 1;
            for _ in 0..E_INK_HEIGHT {
                // SAFETY: the array type fixes the full-frame length, and the
                // 600-by-75-byte traversal consumes each byte once from the end
                // without crossing index zero.
                let dram = unsafe { *self.framebuffer_bw.get_unchecked(position as usize) };
                position -= 1;

                let mut data = waveform_lut[(dram >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                if PANEL_FULL_ROW_START_REFERENCE_SEQUENCE {
                    self.hscan_start_full_refresh_reference(send);
                } else {
                    self.hscan_start_full_refresh(send);
                }

                data = waveform_lut[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                #[cfg(target_arch = "xtensa")]
                if PANEL_FULL_HARDWARE_LOOP {
                    // SAFETY: `position + 1` points one byte past the 74 unread
                    // framebuffer bytes in this row. The helper reads exactly
                    // those bytes, indexes the fixed 16-entry waveform LUT and
                    // 256-entry pin LUT, and writes only the live GPIO registers.
                    unsafe {
                        scan_full_source_words_hoisted(
                            self.framebuffer_bw.as_ptr().add(position as usize + 1),
                            waveform_lut.as_ptr(),
                            self.pin_lut.as_ptr(),
                            GpioFast::out_set_ptr(),
                            GpioFast::out_clear_ptr(),
                            send,
                        );
                    }
                    position -= E_INK_WIDTH as isize / 8 - 1;
                } else {
                    self.write_data_and_clock_full_refresh(send);

                    for _ in 0..(E_INK_WIDTH / 8 - 1) {
                        // SAFETY: same exact-length traversal invariant as above.
                        let dram = unsafe { *self.framebuffer_bw.get_unchecked(position as usize) };
                        position -= 1;

                        data = waveform_lut[(dram >> 4) as usize];
                        send = self.pin_lut[data as usize];
                        self.write_data_and_clock_full_refresh(send);

                        data = waveform_lut[(dram & 0x0F) as usize];
                        send = self.pin_lut[data as usize];
                        self.write_data_and_clock_full_refresh(send);
                    }

                    self.write_data_and_clock_full_refresh(send);
                }

                #[cfg(not(target_arch = "xtensa"))]
                {
                    self.write_data_and_clock_full_refresh(send);

                    for _ in 0..(E_INK_WIDTH / 8 - 1) {
                        // SAFETY: same exact-length traversal invariant as above.
                        let dram = unsafe { *self.framebuffer_bw.get_unchecked(position as usize) };
                        position -= 1;

                        data = waveform_lut[(dram >> 4) as usize];
                        send = self.pin_lut[data as usize];
                        self.write_data_and_clock_full_refresh(send);

                        data = waveform_lut[(dram & 0x0F) as usize];
                        send = self.pin_lut[data as usize];
                        self.write_data_and_clock_full_refresh(send);
                    }

                    self.write_data_and_clock_full_refresh(send);
                }
                self.vscan_end_full_refresh();
            }
            debug_assert_eq!(position, -1);
        });
    }
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
unsafe fn scan_full_source_words_hoisted(
    source: *const u8,
    waveform_lut: *const u8,
    pin_lut: *const u32,
    out_set: *mut u32,
    out_clear: *mut u32,
    first_send: u32,
) {
    if PANEL_FULL_PIPELINED_HOLD {
        // SAFETY: identical caller-owned pointer and GPIO invariants to this
        // helper; only the work/hold instruction schedule differs.
        unsafe {
            scan_full_source_words_pipelined(
                source,
                waveform_lut,
                pin_lut,
                out_set,
                out_clear,
                first_send,
            );
        }
        return;
    }
    if PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED {
        // SAFETY: identical caller-owned pointer and GPIO invariants to this
        // helper; only the inner pulse hold implementation differs.
        unsafe {
            scan_full_source_words_fixed_hold(
                source,
                waveform_lut,
                pin_lut,
                out_set,
                out_clear,
                first_send,
            );
        }
        return;
    }

    const SOURCE_BYTES_PER_ROW: usize = E_INK_WIDTH / 8 - 1;
    const _: () = assert!(
        !PANEL_FULL_HARDWARE_LOOP
            || (!PANEL_FULL_FIXED_HOLD_SELECTED && !PANEL_FULL_REFERENCE_SEQUENCE)
    );

    let source_bytes = SOURCE_BYTES_PER_ROW;
    let clear_mask = DATA_MASK | CL_MASK;
    // SAFETY: the caller supplies one-past pointers for exactly 74 readable
    // framebuffer bytes, fixed-size LUTs, and the live bank-0 W1TS/W1TC
    // registers. This emits the row's lower first nibble, 148 looped nibble
    // pulses, and the required repeated final pulse with one numeric hold each.
    unsafe {
        core::arch::asm!(
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "2:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltui {elapsed}, {hold_cycles}, 2b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "loop {source_bytes}, 5f",
            "addi {source}, {source}, -1",
            "l8ui {pixel}, {source}, 0",
            "srli {send}, {pixel}, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "3:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltui {elapsed}, {hold_cycles}, 3b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "extui {send}, {pixel}, 0, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "4:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltui {elapsed}, {hold_cycles}, 4b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "5:",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "6:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltui {elapsed}, {hold_cycles}, 6b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            source = inout(reg) source => _,
            pixel = out(reg) _,
            send = inout(reg) first_send => _,
            started = out(reg) _,
            elapsed = out(reg) _,
            source_bytes = in(reg) source_bytes,
            waveform_lut = in(reg) waveform_lut,
            pin_lut = in(reg) pin_lut,
            out_set = in(reg) out_set,
            out_clear = in(reg) out_clear,
            cl_mask = in(reg) CL_MASK,
            clear_mask = in(reg) clear_mask,
            hold_cycles = const PANEL_FULL_CL_HIGH_HOLD_CYCLES,
            options(nostack)
        );
    }
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
unsafe fn scan_full_source_words_fixed_hold(
    source: *const u8,
    waveform_lut: *const u8,
    pin_lut: *const u32,
    out_set: *mut u32,
    out_clear: *mut u32,
    first_send: u32,
) {
    const SOURCE_BYTES_PER_ROW: usize = E_INK_WIDTH / 8 - 1;
    const _: () = assert!(
        !PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED
            || (PANEL_FULL_HARDWARE_LOOP
                && !PANEL_FULL_PIPELINED_HOLD
                && !PANEL_FULL_FIXED_HOLD_SELECTED
                && !PANEL_FULL_REFERENCE_SEQUENCE)
    );

    let source_bytes = SOURCE_BYTES_PER_ROW;
    let clear_mask = DATA_MASK | CL_MASK;
    // SAFETY: the caller supplies one-past pointers for exactly 74 readable
    // framebuffer bytes, fixed-size LUTs, and the live bank-0 W1TS/W1TC
    // registers. This keeps one identical fixed-padding run between every
    // inner CL set and clear; the row-start pulse remains on the production
    // full-refresh CCOUNT helper.
    unsafe {
        core::arch::asm!(
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            ".rept {hold_nops}",
            "nop",
            ".endr",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "loop {source_bytes}, 5f",
            "addi {source}, {source}, -1",
            "l8ui {pixel}, {source}, 0",
            "srli {send}, {pixel}, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            ".rept {hold_nops}",
            "nop",
            ".endr",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "extui {send}, {pixel}, 0, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            ".rept {hold_nops}",
            "nop",
            ".endr",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "5:",
            "memw",
            "s32i {send}, {out_set}, 0",
            ".rept {hold_nops}",
            "nop",
            ".endr",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            source = inout(reg) source => _,
            pixel = out(reg) _,
            send = inout(reg) first_send => _,
            source_bytes = in(reg) source_bytes,
            waveform_lut = in(reg) waveform_lut,
            pin_lut = in(reg) pin_lut,
            out_set = in(reg) out_set,
            out_clear = in(reg) out_clear,
            cl_mask = in(reg) CL_MASK,
            clear_mask = in(reg) clear_mask,
            hold_nops = const PANEL_FULL_SOURCE_FIXED_HOLD_NOPS,
            options(nostack)
        );
    }
}

#[cfg(target_arch = "xtensa")]
#[inline(always)]
unsafe fn scan_full_source_words_pipelined(
    source: *const u8,
    waveform_lut: *const u8,
    pin_lut: *const u32,
    out_set: *mut u32,
    out_clear: *mut u32,
    first_send: u32,
) {
    const SOURCE_BYTES_PER_ROW: usize = E_INK_WIDTH / 8 - 1;
    const _: () = assert!(
        !PANEL_FULL_PIPELINED_HOLD
            || (PANEL_FULL_HARDWARE_LOOP
                && !PANEL_FULL_FIXED_HOLD_SELECTED
                && !PANEL_FULL_REFERENCE_SEQUENCE)
    );

    let source_bytes = SOURCE_BYTES_PER_ROW;
    let clear_mask = DATA_MASK | CL_MASK;
    // SAFETY: the caller supplies one-past pointers for exactly 74 readable
    // framebuffer bytes, fixed-size LUTs, and the live bank-0 W1TS/W1TC
    // registers. The next LUT value is prepared while the current CL pulse is
    // high, but every clear remains guarded by the 12-cycle CCOUNT threshold.
    unsafe {
        core::arch::asm!(
            "or {send}, {send}, {cl_mask}",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "loop {source_bytes}, 5f",
            // Prepare the upper nibble while the preceding lower-nibble pulse
            // remains high.
            "addi {source}, {source}, -1",
            "l8ui {pixel}, {source}, 0",
            "srli {send}, {pixel}, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "2:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltu {elapsed}, {hold_cycles}, 2b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            // Prepare the lower nibble while the upper-nibble pulse is high.
            "extui {send}, {pixel}, 0, 4",
            "add {send}, {send}, {waveform_lut}",
            "l8ui {send}, {send}, 0",
            "addx4 {send}, {send}, {pin_lut}",
            "l32i {send}, {send}, 0",
            "or {send}, {send}, {cl_mask}",
            "3:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltu {elapsed}, {hold_cycles}, 3b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "5:",
            // Complete the final lower-nibble pulse, then emit the required
            // repeated final pulse using the same data and threshold.
            "4:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltu {elapsed}, {hold_cycles}, 4b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            "memw",
            "s32i {send}, {out_set}, 0",
            "rsr.ccount {started}",
            "6:",
            "rsr.ccount {elapsed}",
            "sub {elapsed}, {elapsed}, {started}",
            "bltu {elapsed}, {hold_cycles}, 6b",
            "memw",
            "s32i {clear_mask}, {out_clear}, 0",
            source = inout(reg) source => _,
            pixel = out(reg) _,
            send = inout(reg) first_send => _,
            started = out(reg) _,
            elapsed = out(reg) _,
            source_bytes = in(reg) source_bytes,
            waveform_lut = in(reg) waveform_lut,
            pin_lut = in(reg) pin_lut,
            out_set = in(reg) out_set,
            out_clear = in(reg) out_clear,
            cl_mask = in(reg) CL_MASK,
            clear_mask = in(reg) clear_mask,
            hold_cycles = in(reg) PANEL_FULL_CL_HIGH_HOLD_CYCLES,
            options(nostack)
        );
    }
}
