#[cfg(feature = "graphics")]
use core::convert::Infallible;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "graphics")]
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Size},
};

use super::{
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

mod base;
mod clean;
mod control;
mod display;
mod graphics;
mod i2c;
mod panel;
mod waveform;
