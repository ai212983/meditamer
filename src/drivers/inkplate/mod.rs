#[cfg(feature = "graphics")]
use core::convert::Infallible;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "graphics")]
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Size},
};

use crate::{
    gpio_fast::{
        GpioFast, CKV_MASK1, CL_MASK, DATA_MASK, LE_MASK, PANEL_OUT1_ENABLE_MASK,
        PANEL_OUT_ENABLE_MASK, SPH_MASK1,
    },
    platform::{DelayOps, I2cOps},
};

pub const E_INK_WIDTH: usize = 600;
pub const E_INK_HEIGHT: usize = 600;
const FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 8;

const IO_INT_ADDR: u8 = 0x20;
const IO_EXT_ADDR: u8 = 0x21;
const TPS65186_ADDR: u8 = 0x48;
const FRONTLIGHT_DIGIPOT_ADDR: u8 = 0x2E;
const BUZZER_DIGIPOT_ADDR: u8 = 0x2F;
const FUEL_GAUGE_ADDR: u8 = 0x55;
const LSM6DS3_ADDR: u8 = 0x6B;
const TOUCHSCREEN_ADDR: u8 = 0x15;
const PWR_GOOD_OK: u8 = 0b1111_1010;
const BQ27441_COMMAND_SOC: u8 = 0x1C;
const LSM6DS3_REG_WHO_AM_I: u8 = 0x0F;
const LSM6DS3_REG_CTRL1_XL: u8 = 0x10;
const LSM6DS3_REG_CTRL2_G: u8 = 0x11;
const LSM6DS3_REG_TAP_SRC: u8 = 0x1C;
const LSM6DS3_REG_OUTX_L_G: u8 = 0x22;
const LSM6DS3_REG_TAP_CFG1: u8 = 0x58;
const LSM6DS3_REG_TAP_THS_6D: u8 = 0x59;
const LSM6DS3_REG_INT_DUR2: u8 = 0x5A;
const LSM6DS3_REG_WAKE_UP_THS: u8 = 0x5B;
const LSM6DS3_REG_MD1_CFG: u8 = 0x5E;
const LSM6DS3_WHO_AM_I_VALUE: u8 = 0x69;
const LSM6DS3_TAP_EVENT_BIT: u8 = 0x40;
const LSM6DS3_DOUBLE_TAP_BIT: u8 = 0x10;

const PCAL_REG_ADDRS: [u8; 23] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4F,
];
const PCAL_OUTPORT0_ARRAY: usize = 2;
const PCAL_OUTPORT1_ARRAY: usize = 3;
const PCAL_CFGPORT0_ARRAY: usize = 6;
const PCAL_CFGPORT1_ARRAY: usize = 7;
const PCAL_PUPDEN_REG0_ARRAY: usize = 14;
const PCAL_PUPDEN_REG1_ARRAY: usize = 15;
const PCAL_PUPDSEL_REG0_ARRAY: usize = 16;
const PCAL_PUPDSEL_REG1_ARRAY: usize = 17;

const OE: u8 = 0;
const GMOD: u8 = 1;
const SPV: u8 = 2;
const WAKEUP: u8 = 3;
const PWRUP: u8 = 4;
const VCOM: u8 = 5;
const GPIO0_ENABLE: u8 = 8;
const INT_APDS: u8 = 9;
const BATTERY_MEAS_EN: u8 = 9;
const FRONTLIGHT_EN: u8 = 10;
const SD_PMOS_PIN: u8 = 11;
const BUZZ_EN: u8 = 12;
const INT2_LSM: u8 = 13;
const INT1_LSM: u8 = 14;
const FG_GPOUT: u8 = 15;
const TOUCHSCREEN_EN: u8 = 0;
const TOUCHSCREEN_RST: u8 = 1;

const TOUCH_SOFT_RESET_CMD: [u8; 4] = [0x77, 0x77, 0x77, 0x77];
const TOUCH_HELLO_PACKET: [u8; 4] = [0x55, 0x55, 0x55, 0x55];
const TOUCH_GET_X_RES_CMD: [u8; 4] = [0x53, 0x60, 0x00, 0x00];
const TOUCH_GET_Y_RES_CMD: [u8; 4] = [0x53, 0x63, 0x00, 0x00];
const TOUCH_GET_POWER_STATE_CMD: [u8; 4] = [0x53, 0x50, 0x00, 0x01];
const TOUCH_SOFT_RESET_POLL_INTERVAL_MS: u32 = 20;
const TOUCH_SOFT_RESET_TIMEOUT_MS: u32 = 1_000;

const BEEP_FREQ_MIN_HZ: i32 = 572;
const BEEP_FREQ_MAX_HZ: i32 = 2933;
const ENABLE_BUZZER_PITCH_CONTROL: bool = true;

const LUT2: [u8; 16] = [
    0xAA, 0xA9, 0xA6, 0xA5, 0x9A, 0x99, 0x96, 0x95, 0x6A, 0x69, 0x66, 0x65, 0x5A, 0x59, 0x56, 0x55,
];
const LUTW: [u8; 16] = [
    0xFF, 0xFE, 0xFB, 0xFA, 0xEF, 0xEE, 0xEB, 0xEA, 0xBF, 0xBE, 0xBB, 0xBA, 0xAF, 0xAE, 0xAB, 0xAA,
];
const LUTB: [u8; 16] = [
    0xFF, 0xFD, 0xF7, 0xF5, 0xDF, 0xDD, 0xD7, 0xD5, 0x7F, 0x7D, 0x77, 0x75, 0x5F, 0x5D, 0x57, 0x55,
];

static FRAMEBUFFER_TAKEN: AtomicBool = AtomicBool::new(false);
#[unsafe(link_section = ".dram2_uninit")]
static mut FRAMEBUFFER_BW: MaybeUninit<[u8; FRAMEBUFFER_BYTES]> = MaybeUninit::uninit();
#[unsafe(link_section = ".dram2_uninit")]
static mut PREVIOUS_BW: MaybeUninit<[u8; FRAMEBUFFER_BYTES]> = MaybeUninit::uninit();

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum PinMode {
    Input,
    Output,
    InputPullUp,
    InputPullDown,
}

#[derive(Clone, Copy, Debug)]
pub enum TestPattern {
    CheckerboardDiagonals,
    VerticalBars,
    HorizontalBars,
    SolidBlack,
    SolidWhite,
}

#[derive(Debug)]
pub enum InkplateHalError<E> {
    I2c(E),
    InvalidPin(u8),
    UnsupportedAddress(u8),
    FramebufferInUse,
    PanelPowerTimeout(u8),
}

impl<E> From<E> for InkplateHalError<E> {
    fn from(value: E) -> Self {
        Self::I2c(value)
    }
}

#[derive(Clone, Copy)]
pub struct ProbeStatus {
    pub io_internal: bool,
    pub io_external: bool,
    pub tps65186: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum TouchInitStatus {
    Ready { x_res: u16, y_res: u16 },
    HelloMismatch { hello: [u8; 4] },
    ZeroResolution { x_res: u16, y_res: u16 },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TouchPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TouchSample {
    pub touch_count: u8,
    pub points: [TouchPoint; 2],
    pub raw: [u8; 8],
}

#[derive(Clone, Copy)]
pub struct DebugSnapshot {
    pub pcal_out0: u8,
    pub pcal_out1: u8,
    pub pcal_cfg0: u8,
    pub pcal_cfg1: u8,
}

pub type Result<T, E> = core::result::Result<T, InkplateHalError<E>>;

pub struct InkplateHal<I2C, D> {
    i2c: I2C,
    delay: D,
    io_regs_int: [u8; 23],
    io_regs_ext: [u8; 23],
    battery_gate_active_high: Option<bool>,
    touch_x_res: u16,
    touch_y_res: u16,
    pin_lut: [u32; 256],
    panel_fast_ready: bool,
    panel_on: bool,
    framebuffer_bw: &'static mut [u8; FRAMEBUFFER_BYTES],
    previous_bw: &'static mut [u8; FRAMEBUFFER_BYTES],
}

impl<I2C, D> Drop for InkplateHal<I2C, D> {
    fn drop(&mut self) {
        FRAMEBUFFER_TAKEN.store(false, Ordering::Release);
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn new(i2c: I2C, delay: D) -> Result<Self, I2C::Error> {
        if FRAMEBUFFER_TAKEN.swap(true, Ordering::AcqRel) {
            return Err(InkplateHalError::FramebufferInUse);
        }

        let mut pin_lut = [0u32; 256];
        for (i, slot) in pin_lut.iter_mut().enumerate() {
            let v = i as u8;
            *slot = (((v & 0b0000_0011) as u32) << 4)
                | ((((v & 0b0000_1100) >> 2) as u32) << 18)
                | ((((v & 0b0001_0000) >> 4) as u32) << 23)
                | ((((v & 0b1110_0000) >> 5) as u32) << 25);
        }

        let framebuffer_bw = unsafe {
            let slot = &mut *core::ptr::addr_of_mut!(FRAMEBUFFER_BW);
            core::ptr::write_bytes(slot.as_mut_ptr().cast::<u8>(), 0, FRAMEBUFFER_BYTES);
            slot.assume_init_mut()
        };
        let previous_bw = unsafe {
            let slot = &mut *core::ptr::addr_of_mut!(PREVIOUS_BW);
            core::ptr::write_bytes(slot.as_mut_ptr().cast::<u8>(), 0, FRAMEBUFFER_BYTES);
            slot.assume_init_mut()
        };

        Ok(Self {
            i2c,
            delay,
            io_regs_int: [0; 23],
            io_regs_ext: [0; 23],
            battery_gate_active_high: None,
            touch_x_res: 0,
            touch_y_res: 0,
            pin_lut,
            panel_fast_ready: false,
            panel_on: false,
            framebuffer_bw,
            previous_bw,
        })
    }

    pub fn width(&self) -> usize {
        E_INK_WIDTH
    }

    pub fn height(&self) -> usize {
        E_INK_HEIGHT
    }

    pub fn probe_devices(&mut self) -> ProbeStatus {
        ProbeStatus {
            io_internal: self.read_i2c_reg(IO_INT_ADDR, 0x00).is_ok(),
            io_external: self.read_i2c_reg(IO_EXT_ADDR, 0x00).is_ok(),
            tps65186: self.read_i2c_reg(TPS65186_ADDR, 0x0F).is_ok(),
        }
    }

    pub fn init_core(&mut self) -> Result<(), I2C::Error> {
        self.io_begin(IO_INT_ADDR)?;
        self.io_begin(IO_EXT_ADDR)?;

        self.pin_mode_internal(IO_INT_ADDR, VCOM, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, PWRUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, WAKEUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GPIO0_ENABLE, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, GPIO0_ENABLE, true)?;
        self.pin_mode_internal(IO_INT_ADDR, INT_APDS, PinMode::InputPullUp)?;
        self.pin_mode_internal(IO_INT_ADDR, INT2_LSM, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, INT1_LSM, PinMode::Input)?;

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        self.delay.delay_ms(1);
        self.i2c_write(
            TPS65186_ADDR,
            &[0x09, 0b0001_1011, 0b0000_0000, 0b0001_1011, 0b0000_0000],
        )?;
        self.delay.delay_ms(1);
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;

        self.pin_mode_internal(IO_INT_ADDR, FRONTLIGHT_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)?;
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, true)?;
        self.pin_mode_internal(IO_INT_ADDR, BUZZ_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)?;
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, PinMode::Output)?;
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, PinMode::Output)?;
        // Touchscreen power-enable is active-low; keep it off by default.
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, true)?;
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, true)?;

        Ok(())
    }

    pub fn clear_bw(&mut self) {
        self.framebuffer_bw.fill(0);
    }

    pub fn set_pixel_bw(&mut self, x: usize, y: usize, black: bool) {
        if x >= E_INK_WIDTH || y >= E_INK_HEIGHT {
            return;
        }

        // Panel scan order is currently 90deg CCW from logical coordinates.
        // Rotate logical pixels CW into the backing buffer to compensate.
        let panel_x = E_INK_WIDTH - 1 - y;
        let panel_y = x;

        let byte_idx = (E_INK_WIDTH / 8) * panel_y + (panel_x / 8);
        let bit = 1u8 << (panel_x % 8);
        if black {
            self.framebuffer_bw[byte_idx] |= bit;
        } else {
            self.framebuffer_bw[byte_idx] &= !bit;
        }
    }

    pub fn draw_test_pattern(&mut self, pattern: TestPattern) {
        self.clear_bw();
        match pattern {
            TestPattern::CheckerboardDiagonals => {
                for y in 0..E_INK_HEIGHT {
                    for x in 0..E_INK_WIDTH {
                        let checker = ((x / 24) + (y / 24)) % 2 == 0;
                        let diag_a = x == y;
                        let diag_b = x + y == E_INK_WIDTH - 1;
                        if checker || diag_a || diag_b {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::VerticalBars => {
                for y in 0..E_INK_HEIGHT {
                    for x in 0..E_INK_WIDTH {
                        if (x / 50) % 2 == 0 {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::HorizontalBars => {
                for y in 0..E_INK_HEIGHT {
                    if (y / 50) % 2 == 0 {
                        for x in 0..E_INK_WIDTH {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::SolidBlack => self.framebuffer_bw.fill(0xFF),
            TestPattern::SolidWhite => self.framebuffer_bw.fill(0x00),
        }
    }
}

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

type Lsm6ds3MotionRaw = (i16, i16, i16, i16, i16, i16);

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn set_brightness(&mut self, brightness: u8) -> Result<(), I2C::Error> {
        let _ = self.set_brightness_checked(brightness)?;
        Ok(())
    }

    pub fn set_brightness_checked(&mut self, brightness: u8) -> Result<bool, I2C::Error> {
        let mut prep_ok = false;
        for prep_attempt in 0..5u32 {
            let wake_ok = self.set_wakeup(true).is_ok();
            self.delay.delay_ms(2);
            let frontlight_ok = self.frontlight_on().is_ok();
            self.delay.delay_ms(4);
            if wake_ok && frontlight_ok {
                prep_ok = true;
                break;
            }

            let _ = self.frontlight_off();
            let _ = self.i2c.reset();
            self.delay.delay_ms(2 + prep_attempt * 2);
        }
        if !prep_ok {
            return Ok(false);
        }

        let cmd = [0x00, 63u8.saturating_sub(brightness & 0b0011_1111)];
        for attempt in 0..8u32 {
            if self.i2c_write(FRONTLIGHT_DIGIPOT_ADDR, &cmd).is_ok() {
                return Ok(true);
            }
            if attempt == 2 {
                let _ = self.frontlight_off();
                self.delay.delay_ms(3);
                let _ = self.frontlight_on();
                self.delay.delay_ms(5);
            }
            self.delay.delay_ms(2 + attempt * 2);
        }
        Ok(false)
    }

    pub fn frontlight_on(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, true)
    }

    pub fn frontlight_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)
    }

    pub fn buzzer_on(&mut self, freq_hz: i32) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, false)?;
        self.delay.delay_ms(1);
        if self.set_buzzer_frequency(freq_hz).is_err() {
            let _ = self.buzzer_off();
        }
        Ok(())
    }

    pub fn buzzer_off(&mut self) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)
    }

    pub fn beep(&mut self, length_ms: u32, freq_hz: i32) -> Result<(), I2C::Error> {
        self.buzzer_on(freq_hz)?;
        self.delay.delay_ms(length_ms);
        self.buzzer_off()
    }

    pub fn read_power_good(&mut self) -> Result<u8, I2C::Error> {
        self.read_i2c_reg(TPS65186_ADDR, 0x0F)
    }

    pub fn set_wakeup(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, enabled)
    }

    pub fn sd_card_power_on(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, false)?;
        self.delay.delay_ms(50);
        Ok(())
    }

    pub fn sd_card_power_off(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Input)
    }

    pub fn touch_power_enabled(&mut self, enabled: bool) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, PinMode::Output)?;
        // Touchscreen power-enable is active-low on Inkplate 4 TEMPERA.
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, !enabled)
    }

    pub fn touch_hardware_reset(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, PinMode::Output)?;
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, false)?;
        self.delay.delay_ms(30);
        self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_RST, true)?;
        self.delay.delay_ms(30);
        Ok(())
    }

    pub fn touch_software_reset(&mut self) -> Result<bool, I2C::Error> {
        Ok(self.touch_software_reset_read_hello()? == TOUCH_HELLO_PACKET)
    }

    pub fn touch_software_reset_read_hello(&mut self) -> Result<[u8; 4], I2C::Error> {
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_SOFT_RESET_CMD)?;
        let mut hello = [0u8; 4];
        let attempts = TOUCH_SOFT_RESET_TIMEOUT_MS
            .saturating_div(TOUCH_SOFT_RESET_POLL_INTERVAL_MS)
            .max(1);
        for _ in 0..attempts {
            self.delay.delay_ms(TOUCH_SOFT_RESET_POLL_INTERVAL_MS);
            self.i2c_read(TOUCHSCREEN_ADDR, &mut hello)?;
            if hello == TOUCH_HELLO_PACKET {
                break;
            }
        }
        Ok(hello)
    }

    pub fn touch_set_power_state(&mut self, powered: bool) -> Result<(), I2C::Error> {
        let mut cmd = [0x54, 0x50, 0x00, 0x01];
        if powered {
            cmd[1] |= 1 << 3;
        }
        self.i2c_write(TOUCHSCREEN_ADDR, &cmd)
    }

    pub fn touch_get_power_state(&mut self) -> Result<bool, I2C::Error> {
        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_POWER_STATE_CMD)?;
        let mut response = [0u8; 4];
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        Ok(((response[1] >> 3) & 1) != 0)
    }

    pub fn touch_read_resolution(&mut self) -> Result<(u16, u16), I2C::Error> {
        let mut response = [0u8; 4];

        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_X_RES_CMD)?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        let x_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);

        self.i2c_write(TOUCHSCREEN_ADDR, &TOUCH_GET_Y_RES_CMD)?;
        self.i2c_read(TOUCHSCREEN_ADDR, &mut response)?;
        let y_res = u16::from(response[2]) | (u16::from(response[3] & 0xF0) << 4);

        Ok((x_res, y_res))
    }

    pub fn touch_init_with_status(&mut self) -> Result<TouchInitStatus, I2C::Error> {
        self.touch_power_enabled(true)?;
        self.delay.delay_ms(180);
        self.touch_hardware_reset()?;

        let mut last_hello = [0u8; 4];
        let mut last_res = (0u16, 0u16);
        for attempt in 0..3u32 {
            let hello = self.touch_software_reset_read_hello()?;
            last_hello = hello;
            if hello != TOUCH_HELLO_PACKET {
                if attempt < 2 {
                    let _ = self.touch_hardware_reset();
                    self.delay.delay_ms(15 + attempt * 10);
                    continue;
                }
                return Ok(TouchInitStatus::HelloMismatch { hello });
            }

            let (x_res, y_res) = self.touch_read_resolution()?;
            last_res = (x_res, y_res);
            if x_res == 0 || y_res == 0 {
                if attempt < 2 {
                    self.delay.delay_ms(10 + attempt * 10);
                    continue;
                }
                return Ok(TouchInitStatus::ZeroResolution { x_res, y_res });
            }

            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
            // Match ELAN reference behavior (`tsInit(true)`): bit3=1 means the
            // controller remains powered in run mode for continuous sampling.
            self.touch_set_power_state(true)?;
            return Ok(TouchInitStatus::Ready { x_res, y_res });
        }

        if last_hello != TOUCH_HELLO_PACKET {
            Ok(TouchInitStatus::HelloMismatch { hello: last_hello })
        } else {
            Ok(TouchInitStatus::ZeroResolution {
                x_res: last_res.0,
                y_res: last_res.1,
            })
        }
    }

    pub fn touch_init(&mut self) -> Result<bool, I2C::Error> {
        Ok(matches!(
            self.touch_init_with_status()?,
            TouchInitStatus::Ready { .. }
        ))
    }

    pub fn touch_shutdown(&mut self) -> Result<(), I2C::Error> {
        let _ = self.touch_set_power_state(false);
        self.touch_power_enabled(false)
    }

    pub fn touch_read_raw_data(&mut self) -> Result<[u8; 8], I2C::Error> {
        let mut raw = [0u8; 8];
        self.i2c_read(TOUCHSCREEN_ADDR, &mut raw)?;
        Ok(raw)
    }

    pub fn touch_read_sample(&mut self, rotation: u8) -> Result<TouchSample, I2C::Error> {
        if self.touch_x_res == 0 || self.touch_y_res == 0 {
            let (x_res, y_res) = self.touch_read_resolution()?;
            self.touch_x_res = x_res;
            self.touch_y_res = y_res;
        }

        let mut raw = self.touch_read_raw_data()?;
        // Some ELAN frames are all-zero even while a finger is moving. A short
        // in-call retry burst reduces one-frame gesture collapse without
        // changing higher-level gesture thresholds.
        for _ in 0..TOUCH_RAW_EMPTY_RETRY_COUNT {
            if touch_raw_frame_has_contact(&raw, self.touch_x_res, self.touch_y_res) {
                break;
            }
            self.delay.delay_ms(TOUCH_RAW_EMPTY_RETRY_DELAY_MS);
            raw = self.touch_read_raw_data()?;
        }

        let bit_count = (raw[7].count_ones() as u8).min(2);
        let mut raw_points = [(0u16, 0u16); 2];
        for (idx, raw_point) in raw_points.iter_mut().enumerate() {
            let decoded = Self::touch_decode_xy(&raw, idx);
            *raw_point = if touch_raw_point_plausible(
                decoded.0,
                decoded.1,
                self.touch_x_res,
                self.touch_y_res,
            ) {
                decoded
            } else {
                (0, 0)
            };
        }
        // Some samples report the active contact in slot 1 while slot 0 is empty.
        // Promote the valid coordinate to slot 0 so higher layers always get a
        // stable primary point for single-touch gesture tracking.
        if raw_points[0] == (0, 0) && raw_points[1] != (0, 0) {
            raw_points.swap(0, 1);
        }
        // Some idle/no-data reads still report non-zero status bits in raw[7].
        // Prefer decoded coordinate validity to avoid phantom touches.
        let coord_count = raw_points
            .iter()
            .filter(|(x, y)| *x != 0 || *y != 0)
            .count() as u8;
        let touch_count = touch_presence_count(bit_count, coord_count);

        let mut points = [TouchPoint::default(); 2];
        for (idx, point) in points.iter_mut().enumerate() {
            let (x_raw, y_raw) = raw_points[idx];
            *point = if x_raw == 0 && y_raw == 0 {
                TouchPoint::default()
            } else {
                self.touch_transform_point(x_raw, y_raw, rotation)
            };
        }

        Ok(TouchSample {
            touch_count,
            points,
            raw,
        })
    }

    fn touch_decode_xy(raw: &[u8; 8], index: usize) -> (u16, u16) {
        let base = 1 + index * 3;
        let d0 = raw[base];
        let d1 = raw[base + 1];
        let d2 = raw[base + 2];

        let x = (u16::from(d0 & 0xF0) << 4) | u16::from(d1);
        let y = (u16::from(d0 & 0x0F) << 8) | u16::from(d2);
        (x, y)
    }

    fn touch_scale_axis(raw_value: u16, panel_extent: usize, controller_extent: u16) -> u16 {
        if panel_extent == 0 || controller_extent == 0 {
            return 0;
        }

        let panel_extent_u32 = panel_extent as u32;
        let max_value = panel_extent_u32.saturating_sub(1);
        let numerator = u32::from(raw_value)
            .saturating_mul(panel_extent_u32)
            .saturating_sub(1);
        let scaled = numerator / u32::from(controller_extent);
        scaled.min(max_value) as u16
    }

    fn touch_transform_point(&self, x_raw: u16, y_raw: u16, rotation: u8) -> TouchPoint {
        // Inkplate 4 TEMPERA mapping mirrors both axes at rotation 0.
        let sx = Self::touch_scale_axis(x_raw, E_INK_HEIGHT, self.touch_x_res);
        let sy = Self::touch_scale_axis(y_raw, E_INK_WIDTH, self.touch_y_res);
        let max_x = (E_INK_WIDTH.saturating_sub(1)) as u16;
        let max_y = (E_INK_HEIGHT.saturating_sub(1)) as u16;

        match rotation & 0x03 {
            0 => TouchPoint {
                x: sy,
                y: max_y.saturating_sub(sx),
            },
            1 => TouchPoint {
                x: max_y.saturating_sub(sx),
                y: max_x.saturating_sub(sy),
            },
            2 => TouchPoint {
                x: max_x.saturating_sub(sy),
                y: sx,
            },
            _ => TouchPoint { x: sx, y: sy },
        }
    }

    pub fn fuel_gauge_soc(&mut self) -> Result<u16, I2C::Error> {
        self.wake_fuel_gauge()?;
        self.read_i2c_reg_u16_le(FUEL_GAUGE_ADDR, BQ27441_COMMAND_SOC)
    }

    pub fn lsm6ds3_init_double_tap(&mut self) -> Result<bool, I2C::Error> {
        if self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_WHO_AM_I)? != LSM6DS3_WHO_AM_I_VALUE {
            return Ok(false);
        }

        // SparkFun-style setup: 416Hz accel ODR, +/-2g full scale.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_CTRL1_XL, 0x60])?;
        // Enable gyro at 416Hz / 245dps so app-layer can veto taps during large swings.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_CTRL2_G, 0x60])?;
        // Enable tap detection on X/Y/Z and latch interrupt source until TAP_SRC is read.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_TAP_CFG1, 0x0F])?;
        // Medium threshold: detect enclosure taps, suppress very light contact.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_TAP_THS_6D, 0x09])?;
        // Medium shock/quiet/duration windows.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_INT_DUR2, 0x76])?;
        // Enable single-tap event mode so app-layer can classify multi-tap sequences.
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_WAKE_UP_THS, 0x80])?;
        // Route tap and single-tap sources to INT1 (SparkFun reference pattern).
        self.i2c_write(LSM6DS3_ADDR, &[LSM6DS3_REG_MD1_CFG, 0x48])?;

        // Clear any stale latched source.
        let _ = self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_TAP_SRC);
        Ok(true)
    }

    pub fn lsm6ds3_read_tap_src(&mut self) -> Result<u8, I2C::Error> {
        self.read_i2c_reg(LSM6DS3_ADDR, LSM6DS3_REG_TAP_SRC)
    }

    pub fn lsm6ds3_read_motion_raw(&mut self) -> Result<Lsm6ds3MotionRaw, I2C::Error> {
        let mut raw = [0u8; 12];
        self.i2c_write_read(LSM6DS3_ADDR, &[LSM6DS3_REG_OUTX_L_G], &mut raw)?;

        let gx = i16::from_le_bytes([raw[0], raw[1]]);
        let gy = i16::from_le_bytes([raw[2], raw[3]]);
        let gz = i16::from_le_bytes([raw[4], raw[5]]);
        let ax = i16::from_le_bytes([raw[6], raw[7]]);
        let ay = i16::from_le_bytes([raw[8], raw[9]]);
        let az = i16::from_le_bytes([raw[10], raw[11]]);
        Ok((gx, gy, gz, ax, ay, az))
    }

    pub fn lsm6ds3_int1_level(&mut self) -> Result<bool, I2C::Error> {
        self.digital_read_internal(IO_INT_ADDR, INT1_LSM)
    }

    pub fn lsm6ds3_int2_level(&mut self) -> Result<bool, I2C::Error> {
        self.digital_read_internal(IO_INT_ADDR, INT2_LSM)
    }

    pub fn lsm6ds3_poll_double_tap(&mut self) -> Result<bool, I2C::Error> {
        let tap_src = self.lsm6ds3_read_tap_src()?;
        Ok((tap_src & LSM6DS3_DOUBLE_TAP_BIT) != 0)
    }

    pub fn lsm6ds3_poll_any_tap(&mut self) -> Result<bool, I2C::Error> {
        let tap_src = self.lsm6ds3_read_tap_src()?;
        Ok((tap_src & LSM6DS3_TAP_EVENT_BIT) != 0)
    }

    pub fn wake_fuel_gauge(&mut self) -> Result<(), I2C::Error> {
        // Inkplate 4 TEMPERA reference wakes BQ27441 via GPOUT pull-up edge.
        self.pin_mode_internal(IO_INT_ADDR, FG_GPOUT, PinMode::InputPullUp)?;
        self.delay.delay_ms(1);
        Ok(())
    }

    pub fn battery_measurement_enable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity()?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, gate_active_high)?;
        self.delay.delay_ms(5);
        Ok(())
    }

    pub fn battery_measurement_disable(&mut self) -> Result<(), I2C::Error> {
        let gate_active_high = self.detect_battery_gate_polarity()?;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)
    }

    fn detect_battery_gate_polarity(&mut self) -> Result<bool, I2C::Error> {
        if let Some(gate_active_high) = self.battery_gate_active_high {
            return Ok(gate_active_high);
        }

        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Input)?;
        let idle_state_high = self.digital_read_internal(IO_INT_ADDR, BATTERY_MEAS_EN)?;
        self.pin_mode_internal(IO_INT_ADDR, BATTERY_MEAS_EN, PinMode::Output)?;

        // Arduino reference uses the level observed while floating to detect board revision.
        // If pin reads low, gate is enabled by driving high on newer revisions.
        let gate_active_high = !idle_state_high;
        self.digital_write_internal(IO_INT_ADDR, BATTERY_MEAS_EN, !gate_active_high)?;
        self.battery_gate_active_high = Some(gate_active_high);
        Ok(gate_active_high)
    }

    pub fn debug_snapshot(&mut self) -> Result<DebugSnapshot, I2C::Error> {
        Ok(DebugSnapshot {
            pcal_out0: self.read_i2c_reg(IO_INT_ADDR, 0x02)?,
            pcal_out1: self.read_i2c_reg(IO_INT_ADDR, 0x03)?,
            pcal_cfg0: self.read_i2c_reg(IO_INT_ADDR, 0x06)?,
            pcal_cfg1: self.read_i2c_reg(IO_INT_ADDR, 0x07)?,
        })
    }
}

fn touch_raw_frame_has_contact(raw: &[u8; 8], x_res: u16, y_res: u16) -> bool {
    if raw[7].count_ones() > 0 {
        return true;
    }
    for idx in 0..2 {
        let base = 1 + idx * 3;
        let d0 = raw[base];
        let d1 = raw[base + 1];
        let d2 = raw[base + 2];
        let x_raw = (u16::from(d0 & 0xF0) << 4) | u16::from(d1);
        let y_raw = (u16::from(d0 & 0x0F) << 8) | u16::from(d2);
        if touch_raw_point_plausible(x_raw, y_raw, x_res, y_res) {
            return true;
        }
    }
    false
}

fn touch_raw_point_plausible(x_raw: u16, y_raw: u16, x_res: u16, y_res: u16) -> bool {
    if x_raw == 0 || y_raw == 0 {
        return false;
    }
    if x_res == 0 || y_res == 0 {
        return false;
    }
    // Controller occasionally emits out-of-range garbage while idle.
    // Clamp-to-edge transforms of those samples create phantom touches.
    x_raw <= x_res && y_raw <= y_res
}

const TOUCH_RAW_EMPTY_RETRY_COUNT: u8 = 2;
const TOUCH_RAW_EMPTY_RETRY_DELAY_MS: u32 = 1;

fn touch_presence_count(bit_count: u8, coord_count: u8) -> u8 {
    // Reference stack keeps contact presence from status bits. Keep that signal
    // so coordinate dropouts do not truncate an active swipe.
    bit_count.max(coord_count).min(2)
}

#[cfg(test)]
mod touch_raw_point_plausible_tests {
    use super::{touch_presence_count, touch_raw_frame_has_contact, touch_raw_point_plausible};

    #[test]
    fn rejects_zero_axes() {
        assert!(!touch_raw_point_plausible(0, 100, 2048, 2048));
        assert!(!touch_raw_point_plausible(100, 0, 2048, 2048));
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(!touch_raw_point_plausible(2049, 100, 2048, 2048));
        assert!(!touch_raw_point_plausible(100, 3000, 2048, 2048));
    }

    #[test]
    fn accepts_in_range_non_zero_points() {
        assert!(touch_raw_point_plausible(1, 1, 2048, 2048));
        assert!(touch_raw_point_plausible(2048, 2048, 2048, 2048));
    }

    #[test]
    fn presence_requires_bits_and_coords() {
        assert_eq!(touch_presence_count(0, 0), 0);
        // Bit-only presence must be preserved; higher layers debounce and gate it.
        assert_eq!(touch_presence_count(1, 0), 1);
        // Bit count may flicker low while coordinates are still valid.
        assert_eq!(touch_presence_count(0, 1), 1);
        assert_eq!(touch_presence_count(1, 1), 1);
        assert_eq!(touch_presence_count(2, 1), 1);
        assert_eq!(touch_presence_count(1, 2), 2);
        assert_eq!(touch_presence_count(2, 2), 2);
    }

    #[test]
    fn raw_frame_has_contact_when_status_bits_are_set() {
        let mut raw = [0u8; 8];
        raw[7] = 0x01;
        assert!(touch_raw_frame_has_contact(&raw, 2048, 2048));
    }

    #[test]
    fn raw_frame_has_contact_when_decoded_coordinate_is_plausible() {
        let mut raw = [0u8; 8];
        raw[1] = 0x14; // x_high=0x1, y_high=0x4
        raw[2] = 0x23; // x_low
        raw[3] = 0x56; // y_low
        assert!(touch_raw_frame_has_contact(&raw, 2048, 2048));
    }

    #[test]
    fn raw_frame_without_bits_or_coords_is_empty() {
        let raw = [0u8; 8];
        assert!(!touch_raw_frame_has_contact(&raw, 2048, 2048));
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub fn prepare_panel_fast_io(&mut self) -> Result<(), I2C::Error> {
        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Output)?;

        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;

        GpioFast::out_enable_set(PANEL_OUT_ENABLE_MASK);
        GpioFast::out_enable1_set(PANEL_OUT1_ENABLE_MASK);
        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(true);

        self.panel_fast_ready = true;
        Ok(())
    }

    pub fn panel_fast_waveform_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        let word_a = self.pin_lut[0xAA];
        let word_5 = self.pin_lut[0x55];
        for _ in 0..8 {
            self.write_data_and_clock(word_a);
            self.write_data_and_clock(word_5);
        }
        self.set_le(true);
        self.set_le(false);
        self.set_ckv(true);
        self.set_ckv(false);
        Ok(())
    }

    pub fn panel_waveform_primitives_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        self.vscan_start()?;
        for _ in 0..8 {
            self.hscan_start(self.pin_lut[0xAA]);
            self.pulse_cl_only();
            self.pulse_cl_only();
            self.vscan_end();
        }
        self.set_ckv(false);
        self.set_le(false);
        self.set_sph(true);
        self.set_cl(false);
        Ok(())
    }

    pub fn panel_clean_smoke(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_fast_ready {
            self.prepare_panel_fast_io()?;
        }
        let send = self.pin_lut[0b1010_1010];
        self.vscan_start()?;
        for _ in 0..8 {
            self.hscan_start(send);
            GpioFast::out_set(send | CL_MASK);
            GpioFast::out_clear(DATA_MASK | CL_MASK);
            for _ in 0..8 {
                self.pulse_cl_only();
                self.pulse_cl_only();
            }
            GpioFast::out_set(send | CL_MASK);
            GpioFast::out_clear(DATA_MASK | CL_MASK);
            self.vscan_end();
        }
        self.delay.delay_us(230);
        self.set_ckv(false);
        self.set_le(false);
        self.set_sph(true);
        self.set_cl(false);
        Ok(())
    }

    pub fn eink_on(&mut self) -> Result<(), I2C::Error> {
        if self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        self.delay.delay_ms(5);

        self.i2c_write(TPS65186_ADDR, &[0x01, 0b0010_0000])?;
        self.i2c_write(TPS65186_ADDR, &[0x09, 0b1110_0100])?;
        self.i2c_write(TPS65186_ADDR, &[0x0B, 0b0001_1011])?;

        self.prepare_panel_fast_io()?;
        self.set_le(false);
        self.set_cl(false);
        self.set_sph(true);
        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.set_ckv(false);
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, true)?;

        let mut ok = false;
        let mut last_pg = 0u8;
        for _ in 0..250 {
            self.delay.delay_ms(1);
            last_pg = self.read_power_good()?;
            if last_pg == PWR_GOOD_OK {
                ok = true;
                break;
            }
        }
        if !ok {
            let _ = self.eink_off();
            return Err(InkplateHalError::PanelPowerTimeout(last_pg));
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, true)?;
        self.panel_on = true;
        Ok(())
    }

    pub fn eink_off(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, false)?;
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, GMOD, false)?;

        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(false);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, false)?;

        for _ in 0..250 {
            self.delay.delay_ms(1);
            if self.read_power_good()? == 0 {
                break;
            }
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;
        self.i2c_write(TPS65186_ADDR, &[0x01, 0x00])?;
        self.panel_on = false;
        Ok(())
    }

    pub async fn eink_on_async(&mut self) -> Result<(), I2C::Error> {
        if self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        embassy_time::Timer::after_millis(5).await;

        self.i2c_write(TPS65186_ADDR, &[0x01, 0b0010_0000])?;
        self.i2c_write(TPS65186_ADDR, &[0x09, 0b1110_0100])?;
        self.i2c_write(TPS65186_ADDR, &[0x0B, 0b0001_1011])?;

        self.prepare_panel_fast_io()?;
        self.set_le(false);
        self.set_cl(false);
        self.set_sph(true);
        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.set_ckv(false);
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, true)?;

        let mut ok = false;
        let mut last_pg = 0u8;
        for _ in 0..250 {
            embassy_time::Timer::after_millis(1).await;
            last_pg = self.read_power_good()?;
            if last_pg == PWR_GOOD_OK {
                ok = true;
                break;
            }
        }
        if !ok {
            let _ = self.eink_off_async().await;
            return Err(InkplateHalError::PanelPowerTimeout(last_pg));
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, true)?;
        self.panel_on = true;
        Ok(())
    }

    pub async fn eink_off_async(&mut self) -> Result<(), I2C::Error> {
        if !self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, false)?;
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, GMOD, false)?;

        self.clear_data_and_cl_le();
        self.set_ckv(false);
        self.set_sph(false);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, false)?;

        for _ in 0..250 {
            embassy_time::Timer::after_millis(1).await;
            if self.read_power_good()? == 0 {
                break;
            }
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;
        self.i2c_write(TPS65186_ADDR, &[0x01, 0x00])?;
        self.panel_on = false;
        Ok(())
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    fn clean(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start()?;
            for _ in 0..E_INK_HEIGHT {
                self.hscan_start(send);
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(CL_MASK);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(DATA_MASK | CL_MASK);
                self.vscan_end();
            }
            self.delay.delay_us(230);
        }
        Ok(())
    }

    async fn clean_async(&mut self, c: u8, rep: u8) -> Result<(), I2C::Error> {
        let data = match c {
            0 => 0b1010_1010,
            1 => 0b0101_0101,
            2 => 0b0000_0000,
            3 => 0b1111_1111,
            _ => 0,
        };
        let send = self.pin_lut[data as usize];

        for _ in 0..rep {
            self.vscan_start()?;
            for row in 0..E_INK_HEIGHT {
                self.hscan_start(send);
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(CL_MASK);
                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }
                GpioFast::out_set(send | CL_MASK);
                GpioFast::out_clear(DATA_MASK | CL_MASK);
                self.vscan_end();

                if (row & 0x1F) == 0 {
                    embassy_time::Timer::after_micros(0).await;
                }
            }
            embassy_time::Timer::after_micros(230).await;
        }
        Ok(())
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    fn cached_regs_mut(&mut self, addr: u8) -> Result<&mut [u8; 23], I2C::Error> {
        match addr {
            IO_INT_ADDR => Ok(&mut self.io_regs_int),
            IO_EXT_ADDR => Ok(&mut self.io_regs_ext),
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    fn cached_regs(&self, addr: u8) -> Result<&[u8; 23], I2C::Error> {
        match addr {
            IO_INT_ADDR => Ok(&self.io_regs_int),
            IO_EXT_ADDR => Ok(&self.io_regs_ext),
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    fn io_begin(&mut self, addr: u8) -> Result<(), I2C::Error> {
        let mut regs = [0u8; 23];
        self.i2c_write_read(addr, &[0x00], &mut regs)?;
        match addr {
            IO_INT_ADDR => {
                self.io_regs_int = regs;
                Ok(())
            }
            IO_EXT_ADDR => {
                self.io_regs_ext = regs;
                Ok(())
            }
            _ => Err(InkplateHalError::UnsupportedAddress(addr)),
        }
    }

    fn pin_mode_internal(&mut self, addr: u8, pin: u8, mode: PinMode) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

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
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.write_cached_reg(addr, cfg_idx)?;
            }
            PinMode::Output => {
                self.modify_cached_reg(addr, cfg_idx, 0, 1u8 << bit)?;
                self.modify_cached_reg(addr, out_idx, 0, 1u8 << bit)?;
                self.write_cached_reg(addr, out_idx)?;
                self.write_cached_reg(addr, cfg_idx)?;
            }
            PinMode::InputPullUp => {
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupden_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupdsel_idx, 1u8 << bit, 0)?;
                self.write_cached_reg(addr, cfg_idx)?;
                self.write_cached_reg(addr, pupden_idx)?;
                self.write_cached_reg(addr, pupdsel_idx)?;
            }
            PinMode::InputPullDown => {
                self.modify_cached_reg(addr, cfg_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupden_idx, 1u8 << bit, 0)?;
                self.modify_cached_reg(addr, pupdsel_idx, 0, 1u8 << bit)?;
                self.write_cached_reg(addr, cfg_idx)?;
                self.write_cached_reg(addr, pupden_idx)?;
                self.write_cached_reg(addr, pupdsel_idx)?;
            }
        }
        Ok(())
    }

    fn digital_write_internal(&mut self, addr: u8, pin: u8, state: bool) -> Result<(), I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let out_idx = if port == 0 {
            PCAL_OUTPORT0_ARRAY
        } else {
            PCAL_OUTPORT1_ARRAY
        };
        if state {
            self.modify_cached_reg(addr, out_idx, 1u8 << bit, 0)?;
        } else {
            self.modify_cached_reg(addr, out_idx, 0, 1u8 << bit)?;
        }
        self.write_cached_reg(addr, out_idx)?;
        Ok(())
    }

    fn digital_read_internal(&mut self, addr: u8, pin: u8) -> Result<bool, I2C::Error> {
        if pin > 15 {
            return Err(InkplateHalError::InvalidPin(pin));
        }
        let _ = self.cached_regs(addr)?;

        let port = (pin / 8) as usize;
        let bit = pin % 8;
        let in_idx = if port == 0 { 0 } else { 1 };
        let value = self.read_i2c_reg(addr, PCAL_REG_ADDRS[in_idx])?;
        Ok((value & (1u8 << bit)) != 0)
    }

    fn modify_cached_reg(
        &mut self,
        addr: u8,
        idx: usize,
        set_mask: u8,
        clear_mask: u8,
    ) -> Result<(), I2C::Error> {
        let regs = self.cached_regs_mut(addr)?;
        regs[idx] |= set_mask;
        regs[idx] &= !clear_mask;
        Ok(())
    }

    fn write_cached_reg(&mut self, addr: u8, idx: usize) -> Result<(), I2C::Error> {
        let reg_value = self.cached_regs(addr)?[idx];
        self.i2c_write(addr, &[PCAL_REG_ADDRS[idx], reg_value])
    }

    fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<(), I2C::Error> {
        match self.i2c.write(addr, bytes) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c.write(addr, bytes).map_err(InkplateHalError::I2c)
            }
        }
    }

    fn i2c_write_read(
        &mut self,
        addr: u8,
        bytes: &[u8],
        buffer: &mut [u8],
    ) -> Result<(), I2C::Error> {
        match self.i2c.write_read(addr, bytes, buffer) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c
                    .write_read(addr, bytes, buffer)
                    .map_err(InkplateHalError::I2c)
            }
        }
    }

    fn i2c_read(&mut self, addr: u8, buffer: &mut [u8]) -> Result<(), I2C::Error> {
        match self.i2c.read(addr, buffer) {
            Ok(()) => Ok(()),
            Err(_) => {
                let _ = self.i2c.reset();
                self.delay.delay_ms(1);
                self.i2c.read(addr, buffer).map_err(InkplateHalError::I2c)
            }
        }
    }

    fn read_i2c_reg(&mut self, addr: u8, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf = [0u8; 1];
        self.i2c_write_read(addr, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    fn read_i2c_reg_u16_le(&mut self, addr: u8, reg: u8) -> Result<u16, I2C::Error> {
        let mut buf = [0u8; 2];
        self.i2c_write_read(addr, &[reg], &mut buf)?;
        Ok(((buf[1] as u16) << 8) | (buf[0] as u16))
    }

    pub fn i2c_fault_recovery_smoke(&mut self, attempts: u8) -> Result<(), I2C::Error> {
        let tries = attempts.max(1);
        for _ in 0..tries {
            // 0x7F is intentionally unused in this design; this forces an address NACK.
            let _ = self.i2c_write(0x7F, &[0x00]);
            self.delay.delay_ms(1);
        }

        let _ = self.read_i2c_reg(IO_INT_ADDR, 0x00)?;
        Ok(())
    }

    fn set_buzzer_frequency(&mut self, freq_hz: i32) -> Result<(), I2C::Error> {
        if !ENABLE_BUZZER_PITCH_CONTROL {
            return Ok(());
        }
        let constrained = freq_hz.clamp(BEEP_FREQ_MIN_HZ, BEEP_FREQ_MAX_HZ);
        let wiper_percent = 156.499_57f32 + (-0.130_347_34f32 * constrained as f32);
        if !(0.0..=100.0).contains(&wiper_percent) {
            return Ok(());
        }
        let wiper_value = (((wiper_percent / 100.0) * 127.0) + 0.5) as u8;
        let payload = [wiper_value & 0x7F];

        for _ in 0..4 {
            if self.i2c_write(BUZZER_DIGIPOT_ADDR, &payload).is_ok() {
                return Ok(());
            }
            let _ = self.frontlight_on();
            let _ = self.i2c.reset();
            self.delay.delay_ms(2);
        }
        Ok(())
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    fn vscan_start(&mut self) -> Result<(), I2C::Error> {
        self.set_ckv(true);
        self.delay.delay_us(7);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(8);
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(18);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(18);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        Ok(())
    }

    fn vscan_end(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
    }

    fn hscan_start(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    fn write_data_and_clock(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    fn pulse_cl_only(&self) {
        GpioFast::out_set(CL_MASK);
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    fn clear_data_and_cl_le(&self) {
        GpioFast::out_clear(DATA_MASK | LE_MASK | CL_MASK);
    }

    #[inline(always)]
    fn set_le(&self, high: bool) {
        if high {
            GpioFast::out_set(LE_MASK);
        } else {
            GpioFast::out_clear(LE_MASK);
        }
    }

    #[inline(always)]
    fn set_cl(&self, high: bool) {
        if high {
            GpioFast::out_set(CL_MASK);
        } else {
            GpioFast::out_clear(CL_MASK);
        }
    }

    #[inline(always)]
    fn set_ckv(&self, high: bool) {
        if high {
            GpioFast::out1_set(CKV_MASK1);
        } else {
            GpioFast::out1_clear(CKV_MASK1);
        }
    }

    #[inline(always)]
    fn set_sph(&self, high: bool) {
        if high {
            GpioFast::out1_set(SPH_MASK1);
        } else {
            GpioFast::out1_clear(SPH_MASK1);
        }
    }
}

#[cfg(feature = "graphics")]
impl<I2C, D> OriginDimensions for InkplateHal<I2C, D> {
    fn size(&self) -> Size {
        Size::new(E_INK_WIDTH as u32, E_INK_HEIGHT as u32)
    }
}

#[cfg(feature = "graphics")]
impl<I2C, D> DrawTarget for InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<It>(&mut self, pixels: It) -> core::result::Result<(), Self::Error>
    where
        It: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 {
                continue;
            }
            self.set_pixel_bw(point.x as usize, point.y as usize, color == BinaryColor::On);
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> core::result::Result<(), Self::Error> {
        if color == BinaryColor::On {
            self.framebuffer_bw.fill(0xFF);
        } else {
            self.framebuffer_bw.fill(0x00);
        }
        Ok(())
    }
}
