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

use board::{DirtyArea, Geometry, Panel, RefreshError, RefreshMode};
use esp_hal::delay::Delay;
use esp_hal::gpio::Output;
use esp_hal::spi::master::Spi;
use esp_hal::Blocking;

/// Logical surface, landscape. The panel is natively portrait; Waveshare's own
/// stack draws in 400x300 and applies U8g2's `R1` rotation, and its LVGL port
/// is initialised at 400x300 too, so landscape is the board's intended viewing
/// orientation. Callers work in these coordinates and never see the native
/// ones.
use core::sync::atomic::{AtomicUsize, Ordering};

/// Payload of the most recent flush, published for the UI task, which cannot
/// borrow the panel because LVGL owns it for the lifetime of the program.
static LAST_FLUSH_BYTES: AtomicUsize = AtomicUsize::new(0);

pub fn last_flush_bytes() -> usize {
    LAST_FLUSH_BYTES.load(Ordering::Relaxed)
}

pub const WIDTH: usize = 400;
pub const HEIGHT: usize = 300;

/// Native panel geometry, portrait. Private to the packing.
const NATIVE_WIDTH: usize = 300;
const NATIVE_HEIGHT: usize = 400;
/// Eight vertically adjacent native pixels per byte.
pub const PAGES: usize = NATIVE_HEIGHT / 8;
pub const FRAMEBUFFER_BYTES: usize = NATIVE_WIDTH * PAGES;

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

/// Inclusive bounding box of changed pixels, in **native** coordinates, since
/// that is what the panel's window commands address.
#[derive(Clone, Copy)]
struct Dirty {
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
}

impl Dirty {
    const EMPTY: Self = Self {
        x0: usize::MAX,
        x1: 0,
        y0: usize::MAX,
        y1: 0,
    };

    fn is_empty(&self) -> bool {
        self.x0 > self.x1 || self.y0 > self.y1
    }

    fn add(&mut self, x: usize, y: usize) {
        self.x0 = self.x0.min(x);
        self.x1 = self.x1.max(x);
        self.y0 = self.y0.min(y);
        self.y1 = self.y1.max(y);
    }

    const fn everything() -> Self {
        Self {
            x0: 0,
            x1: NATIVE_WIDTH - 1,
            y0: 0,
            y1: NATIVE_HEIGHT - 1,
        }
    }
}

/// The panel's two update modes. `0xB2` sets a frame rate for each, so the
/// difference is observable on the TE pin rather than only in current draw.
///
/// u8g2's source is inconsistent about which command is which — one comment
/// calls `0x38` low power — so the board measures it instead of trusting the
/// annotation. See the ADR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerMode {
    High,
    Low,
}

pub struct St7305<'d> {
    dirty: Dirty,
    /// Payload bytes sent by the most recent flush, so callers can see what a
    /// partial refresh saved rather than assume it.
    last_flush_bytes: usize,
    framebuffer: &'d mut [u8; FRAMEBUFFER_BYTES],
    spi: Spi<'d, Blocking>,
    dc: Output<'d>,
    cs: Output<'d>,
    reset: Output<'d>,
    delay: Delay,
}

impl<'d> St7305<'d> {
    pub fn new(
        framebuffer: &'d mut [u8; FRAMEBUFFER_BYTES],
        spi: Spi<'d, Blocking>,
        dc: Output<'d>,
        cs: Output<'d>,
        reset: Output<'d>,
    ) -> Self {
        Self {
            // Nothing has been drawn yet, but nothing on the glass is known
            // either: the first refresh must push everything.
            dirty: Dirty::everything(),
            last_flush_bytes: 0,
            framebuffer,
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

    /// Select the panel's update mode. Takes effect from the next frame.
    pub fn set_power_mode(&mut self, mode: PowerMode) {
        self.command(match mode {
            PowerMode::High => 0x38,
            PowerMode::Low => 0x39,
        });
    }

    /// Payload bytes written by the most recent flush.
    pub fn last_flush_bytes(&self) -> usize {
        self.last_flush_bytes
    }

    /// Set one pixel, in logical coordinates, tracking exactly what changed.
    ///
    /// Prefer this to [`Self::framebuffer_mut`] when drawing a small region:
    /// that one cannot know what the caller touched and so marks everything
    /// dirty, which forces the next refresh to send the whole panel.
    pub fn set_pixel(&mut self, x: usize, y: usize, on: bool) {
        if x >= WIDTH || y >= HEIGHT {
            return;
        }
        set_pixel(self.framebuffer, x, y, on);
        self.dirty.add(HEIGHT - 1 - y, x);
    }

    /// Direct framebuffer access. The caller may write anywhere, so this
    /// conservatively marks the whole panel dirty — a partial refresh after
    /// this is indistinguishable from a full one.
    pub fn framebuffer_mut(&mut self) -> &mut [u8; FRAMEBUFFER_BYTES] {
        self.dirty = Dirty::everything();
        self.framebuffer
    }

    /// Push the whole framebuffer.
    pub fn flush(&mut self) {
        self.dirty = Dirty::everything();
        self.flush_dirty();
    }

    /// Push only the region marked dirty, then clear the mark.
    ///
    /// Both windows narrow, not just the rows: the column address is per
    /// twelve-pixel group, so the range is rounded outward to a group boundary
    /// and everything inside it is sent. Redrawing one label costs the bytes
    /// its bounding box covers instead of all 15,000.
    fn flush_dirty(&mut self) {
        if self.dirty.is_empty() {
            return;
        }
        let dirty = self.dirty;
        self.dirty = Dirty::EMPTY;

        let addr_start = COL_ADDR_BASE + (dirty.x0 / COL_GROUP) as u8;
        let addr_end = COL_ADDR_BASE + (dirty.x1 / COL_GROUP) as u8;
        let groups = (addr_end - addr_start + 1) as usize;
        // Three bytes per column group per sub-row: 12 pixels / 4 per byte.
        let bytes_per_subrow = groups * 3;
        // The packing works from the group boundary, not the dirty edge.
        let first_column = (addr_start - COL_ADDR_BASE) as usize * COL_GROUP;
        let last_column =
            (((addr_end - COL_ADDR_BASE) as usize + 1) * COL_GROUP - 1).min(NATIVE_WIDTH - 1);
        let first_page = dirty.y0 / 8;
        let last_page = dirty.y1 / 8;
        self.last_flush_bytes = (last_page - first_page + 1) * bytes_per_subrow * 4;
        LAST_FLUSH_BYTES.store(self.last_flush_bytes, Ordering::Relaxed);

        let mut packed = [0u8; NATIVE_WIDTH];
        // Copied out because `command_with` borrows self mutably while the
        // framebuffer borrow would still be live.
        let mut row_buffer = [0u8; NATIVE_WIDTH];
        for page in first_page..=last_page {
            // The framebuffer's contract is "bit set = ink", matching the
            // Inkplate and what `set_pixel` and `blit_l8` document. This panel
            // renders a set bit as paper, so polarity is inverted here -- once,
            // at the only place that packs -- rather than by making every
            // caller reason backwards.
            for (destination, source) in row_buffer
                .iter_mut()
                .zip(&self.framebuffer[page * NATIVE_WIDTH..(page + 1) * NATIVE_WIDTH])
            {
                *destination = !*source;
            }
            let row = &row_buffer;

            for sub_row in 0..4 {
                let shift = sub_row * 2;
                let base = sub_row * bytes_per_subrow;
                for (index, column) in (first_column..=last_column).step_by(4).enumerate() {
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

/// Set or clear one pixel, in **logical** landscape coordinates.
///
/// Rotates 90 degrees clockwise into the panel's native portrait frame: a
/// logical top-left pixel lands at native top-right, which is what turns the
/// portrait glass into the landscape surface callers expect.
pub fn set_pixel(framebuffer: &mut [u8; FRAMEBUFFER_BYTES], x: usize, y: usize, on: bool) {
    if x >= WIDTH || y >= HEIGHT {
        return;
    }
    let native_x = HEIGHT - 1 - y;
    let native_y = x;
    let index = (native_y / 8) * NATIVE_WIDTH + native_x;
    let bit = 1u8 << (native_y % 8);
    if on {
        framebuffer[index] |= bit;
    } else {
        framebuffer[index] &= !bit;
    }
}

impl Panel for St7305<'_> {
    fn geometry(&self) -> Geometry {
        Geometry {
            width: WIDTH as u16,
            height: HEIGHT as u16,
        }
    }

    /// Both modes. `Partial` sends only the bounding box of what `blit_l8`
    /// touched since the last refresh, which is what makes redrawing one label
    /// cost a fraction of a full frame.
    fn supports(&self, _mode: RefreshMode) -> bool {
        true
    }

    unsafe fn blit_l8(&mut self, area: DirtyArea, pixels: *const u8) -> bool {
        let width = area.x2 - area.x1 + 1;
        let height = area.y2 - area.y1 + 1;
        if width <= 0 || height <= 0 || pixels.is_null() {
            return false;
        }
        let (Ok(width), Ok(height)) = (usize::try_from(width), usize::try_from(height)) else {
            return false;
        };
        let Some(len) = width.checked_mul(height) else {
            return false;
        };
        let source = unsafe { core::slice::from_raw_parts(pixels, len) };

        for row in 0..height {
            let y = area.y1 + row as i32;
            if y < 0 || y as usize >= HEIGHT {
                continue;
            }
            for column in 0..width {
                let x = area.x1 + column as i32;
                if x < 0 || x as usize >= WIDTH {
                    continue;
                }
                // Same threshold as the Inkplate's blit: below mid-grey is ink.
                let ink = source[row * width + column] < 128;
                set_pixel(self.framebuffer, x as usize, y as usize, ink);
                // Record in native coordinates, matching set_pixel's rotation,
                // because the window commands address the panel that way.
                self.dirty
                    .add(HEIGHT - 1 - y as usize, x as usize);
            }
        }
        true
    }

    fn refresh(&mut self, mode: RefreshMode) -> Result<(), RefreshError> {
        match mode {
            RefreshMode::Full => self.flush(),
            RefreshMode::Partial => self.flush_dirty(),
        }
        Ok(())
    }
}
