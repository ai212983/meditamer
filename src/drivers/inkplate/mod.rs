use core::sync::atomic::Ordering;

use super::{
    gpio_fast::{
        GpioFast, CKV_MASK1, CL_MASK, DATA_MASK, LE_MASK, PANEL_OUT1_ENABLE_MASK,
        PANEL_OUT_ENABLE_MASK, SPH_MASK1,
    },
    platform::{DelayOps, I2cOps},
};

pub const E_INK_WIDTH: usize = 600;
pub const E_INK_HEIGHT: usize = 600;
pub const FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 8;
pub const GRAYSCALE_FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 2;
pub const PARTIAL_TRANSITION_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 4;

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
    PanelPowerDownTimeout(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelRefreshFallback {
    BaselineUnavailable,
    TransitionBufferUnavailable,
    PreviousFramebufferUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelRefreshOutcome {
    Full,
    FullFallback {
        reason: PanelRefreshFallback,
    },
    Partial {
        changed_bytes: usize,
        changed_pixels: u32,
    },
    NoChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelRefreshErrorStage {
    PowerOn,
    Waveform,
    PowerOff,
}

#[derive(Debug)]
pub struct PanelRefreshError<E> {
    pub stage: PanelRefreshErrorStage,
    pub source: InkplateHalError<E>,
}

impl<E> PanelRefreshError<E> {
    fn new(stage: PanelRefreshErrorStage, source: InkplateHalError<E>) -> Self {
        Self { stage, source }
    }
}

pub type PanelRefreshResult<E> = core::result::Result<PanelRefreshOutcome, PanelRefreshError<E>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PartialGateDrainTiming {
    pub scan_rows: usize,
    pub transition_us: u64,
    pub cleanup_zero_us: u64,
    pub cleanup_neutral_finalize_us: u64,
    pub full_fallback: bool,
}

impl PartialGateDrainTiming {
    const fn full_fallback() -> Self {
        Self {
            scan_rows: E_INK_HEIGHT,
            transition_us: 0,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us: 0,
            full_fallback: true,
        }
    }

    const fn no_change() -> Self {
        Self {
            scan_rows: 0,
            transition_us: 0,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us: 0,
            full_fallback: false,
        }
    }

    pub const fn waveform_us(self) -> u64 {
        self.transition_us
            .saturating_add(self.cleanup_zero_us)
            .saturating_add(self.cleanup_neutral_finalize_us)
    }
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
    battery_gate_active_high: Option<bool>,
    pin_lut: [u32; 256],
    panel_fast_ready: bool,
    panel_power_state: panel_lifecycle::PanelPowerState,
    framebuffer_bw: &'static mut [u8; FRAMEBUFFER_BYTES],
    /// Previous frame, used only to diff against `framebuffer_bw` when building
    /// the partial transition. Never read by an interrupt-masked scan pass, so
    /// unlike `framebuffer_bw` it can live in PSRAM; see
    /// docs/development/dram-budget.md.
    framebuffer_bw_previous: Option<&'static mut [u8; FRAMEBUFFER_BYTES]>,
    partial_transition: Option<&'static mut [u8; PARTIAL_TRANSITION_BYTES]>,
    partial_ready: bool,
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
mod hardware;
mod i2c;
pub mod imu;
mod panel;
mod panel_lifecycle;
mod partial_transition;
pub mod touch;
mod waveform;

use hardware::*;
