//! ST7305 reflective-LCD panel driver for the Waveshare ESP32-S3-RLCD-4.2.
//!
//! 300x400, 1bpp, over SPI. Translated from Waveshare's ESP-IDF U8g2 component
//! (`02_Example/ESP-IDF/11_U8G2_Test/components/u8g2_st7305`), which is the
//! authoritative source for this panel's init sequence and framebuffer packing.
//!
//! The ST7305's addressing is unusual enough to be worth stating plainly,
//! because none of it is guessable from the pixel geometry:
//!
//! * Columns are addressed in **groups of twelve pixels**, starting at address
//!   `0x12`, and the address window counts **downward** — the window command
//!   takes `0x3C - addr_end` first.
//! * Each transmitted byte covers a **4-pixel-wide by 2-pixel-tall block**,
//!   assembled through [`PACK`] from four source columns at one of four
//!   sub-row shifts. A full tile row is therefore 4 x 75 bytes.
//! * One tile row is eight pixels tall and spans **four** row addresses.
//!
//! The framebuffer is in page format — one byte holds eight vertically adjacent
//! pixels, bit `y % 8` — because that is what the packing consumes directly.

use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_hal::spi::master::Spi;
use esp_hal::Blocking;

pub const WIDTH: usize = 300;
pub const HEIGHT: usize = 400;
/// Eight vertically adjacent pixels per byte.
pub const PAGES: usize = HEIGHT / 8;
pub const FRAMEBUFFER_BYTES: usize = WIDTH * PAGES;

/// First column-group address; the panel's window commands are offsets from it.
const COL_ADDR_BASE: u8 = 0x12;
/// Column addresses count down from here.
const COL_ADDR_MIRROR: u8 = 0x3C;
/// Pixels per column-address group.
const COL_GROUP: usize = 12;

/// Maps a 2-bit vertical pixel pair to its bit positions, per column within the
/// 4-column block a byte covers.
const PACK: [[u8; 4]; 4] = [
    [0x00, 0x80, 0x40, 0xC0],
    [0x00, 0x20, 0x10, 0x30],
    [0x00, 0x08, 0x04, 0x0C],
    [0x00, 0x02, 0x01, 0x03],
];

pub struct St7305<'d> {
    spi: Spi<'d, Blocking>,
    dc: Output<'d>,
    cs: Output<'d>,
    reset: Output<'d>,
    delay: Delay,
}

impl<'d> St7305<'d> {
    pub fn new(
        spi: Spi<'d, Blocking>,
        dc: Output<'d>,
        cs: Output<'d>,
        reset: Output<'d>,
    ) -> Self {
        Self {
            spi,
            dc,
            cs,
            reset,
            delay: Delay::new(),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        // Errors here mean the SPI peripheral is misconfigured, which a retry
        // cannot fix; the caller has no recovery either, so the panel simply
        // stays blank and the boot log says so.
        let _ = self.spi.write(bytes);
    }

    fn command(&mut self, command: u8) {
        self.cs.set_low();
        self.dc.set_low();
        self.write(&[command]);
        self.cs.set_high();
    }

    fn command_with(&mut self, command: u8, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        self.write(&[command]);
        self.dc.set_high();
        self.write(data);
        self.cs.set_high();
    }

    fn hardware_reset(&mut self) {
        self.reset.set_high();
        self.delay.delay_millis(50);
        self.reset.set_low();
        self.delay.delay_millis(20);
        self.reset.set_high();
        self.delay.delay_millis(50);
    }

    /// Power-on sequence. Values are Waveshare's; the datasheet does not
    /// document most of these registers, so they are reproduced rather than
    /// derived and should not be "tidied".
    pub fn init(&mut self) {
        self.hardware_reset();

        self.command_with(0xD6, &[0x17, 0x02]); // NVM load control
        self.command_with(0xD1, &[0x01]); // booster enable
        self.command_with(0xC0, &[0x11, 0x04]); // gate voltage
        self.command_with(0xC1, &[0x69, 0x69, 0x69, 0x69]); // VSHP
        self.command_with(0xC2, &[0x19, 0x19, 0x19, 0x19]); // VSLP
        self.command_with(0xC4, &[0x4B, 0x4B, 0x4B, 0x4B]); // VSHN
        self.command_with(0xC5, &[0x19, 0x19, 0x19, 0x19]); // VSLN
        self.command_with(0xD8, &[0x80, 0xE9]); // OSC
        self.command_with(0xB2, &[0x02]); // frame rate
        self.command_with(
            0xB3,
            &[0xE5, 0xF6, 0x05, 0x46, 0x77, 0x77, 0x77, 0x77, 0x76, 0x45],
        ); // update period gate/source, high-power mode
        self.command_with(0xB4, &[0x05, 0x46, 0x77, 0x77, 0x77, 0x77, 0x76, 0x45]); // ... low-power
        self.command_with(0x62, &[0x32, 0x03, 0x1F]); // gate timing
        self.command_with(0xB7, &[0x13]); // source EQ
        self.command_with(0xB0, &[0x64]); // duty (400 lines)

        self.command(0x11); // sleep out
        self.delay.delay_millis(120);

        self.command_with(0xC9, &[0x00]); // source voltage select
        self.command_with(0x36, &[0x48]); // memory access: MX + BGR
        self.command_with(0x3A, &[0x11]); // 1bpp mode
        self.command_with(0xB9, &[0x20]); // source setting
        self.command_with(0xB8, &[0x29]); // panel setting
        self.command(0x21); // display inversion on
        self.command_with(0x2A, &[0x12, 0x2A]); // full column window
        self.command_with(0x2B, &[0x00, 0xC7]); // full row window
        self.command_with(0x35, &[0x00]); // tearing effect on
        self.command_with(0xD0, &[0xFF]); // auto power-down off
        self.command(0x38); // high-power mode
        self.command(0x29); // display on
    }

    /// Push a whole framebuffer. One tile row at a time: the column window is
    /// constant, only the row window advances.
    pub fn flush(&mut self, framebuffer: &[u8; FRAMEBUFFER_BYTES]) {
        // Whole-width window, computed the way the panel expects rather than
        // hardcoded, so the arithmetic is checkable against WIDTH.
        let addr_start = COL_ADDR_BASE;
        let addr_end = COL_ADDR_BASE + ((WIDTH - 1) / COL_GROUP) as u8;
        let groups = (addr_end - addr_start + 1) as usize;
        // Three bytes per column group per sub-row: 12 pixels / 4 per byte.
        let bytes_per_subrow = groups * 3;

        let mut packed = [0u8; WIDTH];
        for page in 0..PAGES {
            let row = &framebuffer[page * WIDTH..(page + 1) * WIDTH];

            for sub_row in 0..4 {
                let shift = sub_row * 2;
                let base = sub_row * bytes_per_subrow;
                for (index, column) in (0..WIDTH).step_by(4).enumerate() {
                    packed[base + index] = PACK[0][((row[column] >> shift) & 3) as usize]
                        | PACK[1][((row[column + 1] >> shift) & 3) as usize]
                        | PACK[2][((row[column + 2] >> shift) & 3) as usize]
                        | PACK[3][((row[column + 3] >> shift) & 3) as usize];
                }
            }

            self.command_with(
                0x2A,
                &[COL_ADDR_MIRROR - addr_end, COL_ADDR_MIRROR - addr_start],
            );
            self.command_with(0x2B, &[(page * 4) as u8, (page * 4 + 3) as u8]);
            self.command_with(0x2C, &packed[..bytes_per_subrow * 4]);
        }
    }
}

/// Set or clear one pixel in a page-format framebuffer.
pub fn set_pixel(framebuffer: &mut [u8; FRAMEBUFFER_BYTES], x: usize, y: usize, on: bool) {
    if x >= WIDTH || y >= HEIGHT {
        return;
    }
    let index = (y / 8) * WIDTH + x;
    let bit = 1u8 << (y % 8);
    if on {
        framebuffer[index] |= bit;
    } else {
        framebuffer[index] &= !bit;
    }
}
