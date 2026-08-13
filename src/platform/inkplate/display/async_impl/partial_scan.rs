use super::super::super::{
    DelayOps, I2cOps, InkplateHal, PartialGateDrainTiming, Result, E_INK_HEIGHT, E_INK_WIDTH,
    PARTIAL_TRANSITION_BYTES,
};
use embassy_time::Instant;
use esp_sync::raw::{RawLock, SingleCoreInterruptLock};

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
        scan_rows: usize,
    ) -> Result<PartialGateDrainTiming, I2C::Error> {
        debug_assert!((1..=E_INK_HEIGHT).contains(&scan_rows));
        let transition_started_us = Instant::now().as_micros();
        for _ in 0..9 {
            self.vscan_start().await?;
            let transition = self
                .partial_transition
                .as_deref()
                .expect("partial transition buffer checked before panel power-on");
            self.scan_partial_framebuffer_rows_with_neutral_drain(transition, scan_rows);
            self.delay.delay_us(230);
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
            transition_us,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us,
            full_fallback: false,
        })
    }

    /// Scans one complete partial waveform frame without yielding while panel
    /// row timing is live. The reference drivers only reschedule between
    /// complete 600-row passes; yielding mid-pass collapses horizontal bands.
    #[esp_hal::ram]
    fn scan_partial_framebuffer_pass(&self) {
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

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

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }

    #[esp_hal::ram]
    fn scan_partial_framebuffer_rows_with_neutral_drain(
        &self,
        transition: &[u8; PARTIAL_TRANSITION_BYTES],
        scan_rows: usize,
    ) {
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

        let mut position = PARTIAL_TRANSITION_BYTES;
        for _ in 0..scan_rows {
            position -= 1;
            // SAFETY: transition has the exact full-frame size and the caller
            // constrains scan_rows to 1..=E_INK_HEIGHT, so the loop consumes at
            // most PARTIAL_TRANSITION_BYTES entries.
            let data = unsafe { *transition.get_unchecked(position) };
            let send = self.pin_lut[data as usize];
            self.hscan_start(send);

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
            // state. The 18 us high time matches the reference driver's hidden
            // gate advances in vscan_start().
            let neutral = self.pin_lut[0xFF];
            self.hscan_start(neutral);
            for _ in 0..(E_INK_WIDTH / 4 - 1) {
                self.write_data_and_clock(neutral);
            }
            self.pulse_cl_only();
            self.vscan_end();

            for _ in 1..drain_rows {
                self.set_ckv(true);
                esp_hal::rom::ets_delay_us(18);
                self.vscan_end();
            }
        }

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }
}
