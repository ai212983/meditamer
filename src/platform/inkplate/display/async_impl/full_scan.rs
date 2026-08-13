use super::super::super::{
    DelayOps, I2cOps, InkplateHal, E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BYTES,
    GRAYSCALE_FRAMEBUFFER_BYTES, GRAYSCALE_WAVEFORM_LUT, LUT2, LUTB,
};
use esp_sync::raw::{RawLock, SingleCoreInterruptLock};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    #[esp_hal::ram]
    pub(super) fn scan_grayscale_framebuffer_pass(&self, framebuffer: &[u8], phase: usize) {
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

        let phase_lut = &GRAYSCALE_WAVEFORM_LUT[phase];
        let mut position = GRAYSCALE_FRAMEBUFFER_BYTES as isize;
        for _ in 0..E_INK_HEIGHT {
            position -= 1;
            let upper = phase_lut[framebuffer[position as usize] as usize] << 4;
            position -= 1;
            let lower = phase_lut[framebuffer[position as usize] as usize];
            let mut send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
            self.hscan_start(send);

            position -= 1;
            let upper = phase_lut[framebuffer[position as usize] as usize] << 4;
            position -= 1;
            let lower = phase_lut[framebuffer[position as usize] as usize];
            send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                position -= 1;
                let upper = phase_lut[framebuffer[position as usize] as usize] << 4;
                position -= 1;
                let lower = phase_lut[framebuffer[position as usize] as usize];
                send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                self.write_data_and_clock(send);

                position -= 1;
                let upper = phase_lut[framebuffer[position as usize] as usize] << 4;
                position -= 1;
                let lower = phase_lut[framebuffer[position as usize] as usize];
                send = self.pin_lut[upper as usize] | self.pin_lut[lower as usize];
                self.write_data_and_clock(send);
            }

            self.pulse_cl_only();
            self.vscan_end();
        }
        debug_assert_eq!(position, 0);

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }

    #[esp_hal::ram]
    pub(super) fn scan_full_framebuffer_pass(&self) {
        // The waveform is local to this core and must not use the ESP32
        // critical-section implementation: that implementation also takes a
        // cross-core spinlock. Holding it for an entire 600-row scan can stall
        // the other core for tens of milliseconds and disturb panel timing.
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

        let mut position = FRAMEBUFFER_BYTES as isize - 1;
        for _ in 0..E_INK_HEIGHT {
            let dram = self.framebuffer_bw[position as usize];
            position -= 1;

            let mut data = LUTB[(dram >> 4) as usize];
            let mut send = self.pin_lut[data as usize];
            self.hscan_start(send);

            data = LUTB[(dram & 0x0F) as usize];
            send = self.pin_lut[data as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                let dram = self.framebuffer_bw[position as usize];
                position -= 1;

                data = LUTB[(dram >> 4) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                data = LUTB[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);
            }

            self.write_data_and_clock(send);
            self.vscan_end();
        }
        debug_assert_eq!(position, -1);

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }

    #[esp_hal::ram]
    pub(super) fn scan_full_settle_pass(&self) {
        let interrupt_lock = SingleCoreInterruptLock;
        let interrupt_state = unsafe { interrupt_lock.enter() };

        let mut position = FRAMEBUFFER_BYTES as isize - 1;
        for _ in 0..E_INK_HEIGHT {
            let dram = self.framebuffer_bw[position as usize];
            position -= 1;

            let mut data = LUT2[(dram >> 4) as usize];
            let mut send = self.pin_lut[data as usize];
            self.hscan_start(send);

            data = LUT2[(dram & 0x0F) as usize];
            send = self.pin_lut[data as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                let dram = self.framebuffer_bw[position as usize];
                position -= 1;

                data = LUT2[(dram >> 4) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                data = LUT2[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);
            }

            self.write_data_and_clock(send);
            self.vscan_end();
        }
        debug_assert_eq!(position, -1);

        // SAFETY: paired with the entry above, in the same function and core.
        unsafe { interrupt_lock.exit(interrupt_state) };
    }
}
