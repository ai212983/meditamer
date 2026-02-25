use super::*;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn display_bw(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        self.eink_on()?;
        self.clean(0, 5)?;
        self.clean(1, 15)?;
        self.clean(0, 15)?;
        self.clean(1, 15)?;
        self.clean(0, 15)?;

        for _ in 0..10 {
            let mut ptr = FRAMEBUFFER_BYTES as isize - 1;
            self.vscan_start()?;

            for _ in 0..E_INK_HEIGHT {
                let dram = self.framebuffer_bw[ptr as usize];
                ptr -= 1;

                let mut data = LUTB[(dram >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                self.hscan_start(send);

                data = LUTB[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    let d = self.framebuffer_bw[ptr as usize];
                    ptr -= 1;

                    data = LUTB[(d >> 4) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);

                    data = LUTB[(d & 0x0F) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                }

                self.write_data_and_clock(send);
                self.vscan_end();
            }
            self.delay.delay_us(230);
        }

        let mut pos = FRAMEBUFFER_BYTES as isize - 1;
        self.vscan_start()?;
        for _ in 0..E_INK_HEIGHT {
            let dram = self.framebuffer_bw[pos as usize];
            pos -= 1;

            let mut data = LUT2[(dram >> 4) as usize];
            let mut send = self.pin_lut[data as usize];
            self.hscan_start(send);

            data = LUT2[(dram & 0x0F) as usize];
            send = self.pin_lut[data as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                let d = self.framebuffer_bw[pos as usize];
                pos -= 1;

                data = LUT2[(d >> 4) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                data = LUT2[(d & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);
            }

            self.write_data_and_clock(send);
            self.vscan_end();
        }
        self.delay.delay_us(230);

        self.clean(2, 1)?;
        self.clean(3, 1)?;
        let _ = self.vscan_start();

        if !leave_on {
            self.eink_off()?;
        }

        self.previous_bw.copy_from_slice(self.framebuffer_bw);
        Ok(())
    }

    pub fn display_bw_partial(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        if self.previous_bw == self.framebuffer_bw {
            return Ok(());
        }

        self.eink_on()?;
        // Mirror Inkplate-Arduino partial waveforms: multiple diff passes before cleanup.
        for _ in 0..5 {
            let mut pos = FRAMEBUFFER_BYTES as isize - 1;
            self.vscan_start()?;
            for _ in 0..E_INK_HEIGHT {
                let new = self.framebuffer_bw[pos as usize];
                let old = self.previous_bw[pos as usize];
                pos -= 1;

                let diffw = old & !new;
                let diffb = !old & new;

                let mut data = LUTW[(diffw >> 4) as usize] & LUTB[(diffb >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                self.hscan_start(send);

                data = LUTW[(diffw & 0x0F) as usize] & LUTB[(diffb & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    let new = self.framebuffer_bw[pos as usize];
                    let old = self.previous_bw[pos as usize];
                    pos -= 1;

                    let diffw = old & !new;
                    let diffb = !old & new;

                    data = LUTW[(diffw >> 4) as usize] & LUTB[(diffb >> 4) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);

                    data = LUTW[(diffw & 0x0F) as usize] & LUTB[(diffb & 0x0F) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                }

                self.write_data_and_clock(send);
                self.vscan_end();
            }
            self.delay.delay_us(230);
        }
        self.clean(2, 2)?;
        self.clean(3, 1)?;
        let _ = self.vscan_start();

        if !leave_on {
            self.eink_off()?;
        }

        self.previous_bw.copy_from_slice(self.framebuffer_bw);
        Ok(())
    }

    pub async fn display_bw_async(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        self.eink_on_async().await?;
        self.clean_async(0, 5).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;

        for _ in 0..10 {
            let mut ptr = FRAMEBUFFER_BYTES as isize - 1;
            self.vscan_start()?;

            for row in 0..E_INK_HEIGHT {
                let dram = self.framebuffer_bw[ptr as usize];
                ptr -= 1;

                let mut data = LUTB[(dram >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                self.hscan_start(send);

                data = LUTB[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    let d = self.framebuffer_bw[ptr as usize];
                    ptr -= 1;

                    data = LUTB[(d >> 4) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);

                    data = LUTB[(d & 0x0F) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                }

                self.write_data_and_clock(send);
                self.vscan_end();

                if (row & 0x1F) == 0 {
                    embassy_time::Timer::after_micros(0).await;
                }
            }
            embassy_time::Timer::after_micros(230).await;
        }

        let mut pos = FRAMEBUFFER_BYTES as isize - 1;
        self.vscan_start()?;
        for row in 0..E_INK_HEIGHT {
            let dram = self.framebuffer_bw[pos as usize];
            pos -= 1;

            let mut data = LUT2[(dram >> 4) as usize];
            let mut send = self.pin_lut[data as usize];
            self.hscan_start(send);

            data = LUT2[(dram & 0x0F) as usize];
            send = self.pin_lut[data as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                let d = self.framebuffer_bw[pos as usize];
                pos -= 1;

                data = LUT2[(d >> 4) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                data = LUT2[(d & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);
            }

            self.write_data_and_clock(send);
            self.vscan_end();

            if (row & 0x1F) == 0 {
                embassy_time::Timer::after_micros(0).await;
            }
        }
        embassy_time::Timer::after_micros(230).await;

        self.clean_async(2, 1).await?;
        self.clean_async(3, 1).await?;
        let _ = self.vscan_start();

        if !leave_on {
            self.eink_off_async().await?;
        }

        self.previous_bw.copy_from_slice(self.framebuffer_bw);
        Ok(())
    }

    pub async fn display_bw_partial_async(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        if self.previous_bw == self.framebuffer_bw {
            return Ok(());
        }

        self.eink_on_async().await?;
        // Mirror Inkplate-Arduino partial waveforms: multiple diff passes before cleanup.
        for _ in 0..5 {
            let mut pos = FRAMEBUFFER_BYTES as isize - 1;
            self.vscan_start()?;
            for row in 0..E_INK_HEIGHT {
                let new = self.framebuffer_bw[pos as usize];
                let old = self.previous_bw[pos as usize];
                pos -= 1;

                let diffw = old & !new;
                let diffb = !old & new;

                let mut data = LUTW[(diffw >> 4) as usize] & LUTB[(diffb >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                self.hscan_start(send);

                data = LUTW[(diffw & 0x0F) as usize] & LUTB[(diffb & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    let new = self.framebuffer_bw[pos as usize];
                    let old = self.previous_bw[pos as usize];
                    pos -= 1;

                    let diffw = old & !new;
                    let diffb = !old & new;

                    data = LUTW[(diffw >> 4) as usize] & LUTB[(diffb >> 4) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);

                    data = LUTW[(diffw & 0x0F) as usize] & LUTB[(diffb & 0x0F) as usize];
                    send = self.pin_lut[data as usize];
                    self.write_data_and_clock(send);
                }

                self.write_data_and_clock(send);
                self.vscan_end();

                if (row & 0x1F) == 0 {
                    embassy_time::Timer::after_micros(0).await;
                }
            }
            embassy_time::Timer::after_micros(230).await;
        }
        self.clean_async(2, 2).await?;
        self.clean_async(3, 1).await?;
        let _ = self.vscan_start();

        if !leave_on {
            self.eink_off_async().await?;
        }

        self.previous_bw.copy_from_slice(self.framebuffer_bw);
        Ok(())
    }
}
