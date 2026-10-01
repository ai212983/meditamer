#![no_std]
#![cfg_attr(target_arch = "xtensa", feature(asm_experimental_arch))]
//! Inkplate 4 TEMPERA panel and waveform driver, ESP32 GPIO fast path, I/O
//! expander, board power control, touch and IMU drivers.
//!
//! Extracted from `meditamer`'s `src/platform` by the inkplate-tempera board
//! extraction plan. `meditamer` depends on this crate as a normal path
//! dependency and owns task spawning, policy, persistence, telemetry, UI
//! navigation, and service coordination; this crate owns none of that.

#[cfg(target_os = "none")]
use core::sync::atomic::Ordering;

pub mod environment;
/// Panel-native still-frame container, packings, polarity and rotation.
pub mod frame;
pub mod fuel_gauge;
pub mod panel_blit;
pub mod sht45;
pub mod startup_i2c;

#[cfg(target_os = "none")]
use adapters::DelayOps;
#[cfg(target_os = "none")]
use adapters::I2cOps;
#[cfg(target_os = "none")]
use gpio_fast::{
    GpioFast, CKV_MASK1, CL_MASK, DATA_MASK, LE_MASK, PANEL_OUT1_ENABLE_MASK,
    PANEL_OUT_ENABLE_MASK, SPH_MASK1,
};

pub const E_INK_WIDTH: usize = 600;
pub const E_INK_HEIGHT: usize = 600;
pub const FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 8;
pub const GRAYSCALE_FRAMEBUFFER_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 2;
pub const PARTIAL_TRANSITION_BYTES: usize = E_INK_WIDTH * E_INK_HEIGHT / 4;

#[cfg(target_os = "none")]
use expander::PinMode;

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
    PanelShutdownDeadline,
    WaveformWindowNotQuiescent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanelRefreshFallback {
    BaselineUnavailable,
    TransitionBufferUnavailable,
    PreviousFramebufferUnavailable,
}

/// Failure returned by the strict partial gate-drain API.
#[derive(Debug)]
pub enum PartialGateDrainError<E> {
    /// The panel has no established partial-refresh baseline, or one of the
    /// buffers required to build the partial transition is unavailable.
    NotReady,
    /// The panel transaction reached the driver but failed at this stage.
    Driver(InkplateHalError<E>),
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
    // Constructed only by the hardware-gated `display_bw*_reported_async`
    // family; on a host build with those modules excluded, nothing else
    // calls it.
    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    fn new(stage: PanelRefreshErrorStage, source: InkplateHalError<E>) -> Self {
        Self { stage, source }
    }
}

pub type PanelRefreshResult<E> = core::result::Result<PanelRefreshOutcome, PanelRefreshError<E>>;

// `pub`, not `pub(crate)`: the product's panel-refresh reporting (Phase 1)
// reads this outcome across the crate boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartialGateDrainTiming {
    pub scan_rows: usize,
    pub first_changed_row: usize,
    pub last_changed_row: usize,
    pub changed_span_rows: usize,
    pub source_skip_candidate_rows: usize,
    pub row_discovery_us: u64,
    pub transition_prepare_us: u64,
    pub power_on_us: u64,
    pub transition_us: u64,
    pub vscan_start_us: u64,
    pub source_scan_us: u64,
    pub pass_delay_us: u64,
    pub cleanup_zero_us: u64,
    pub cleanup_neutral_finalize_us: u64,
    pub finalization_us: u64,
    pub previous_copy_us: u64,
    pub full_fallback: bool,
}

/// Phase-level timing returned only by the explicit full-refresh probe API.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FullRefreshTiming {
    pub preparation_us: u64,
    pub power_on_us: u64,
    pub initial_clean_us: u64,
    pub framebuffer_us: u64,
    pub settle_us: u64,
    pub final_clean_us: u64,
    pub terminal_vscan_us: u64,
    /// Diagnostic subtotal for the nonterminal vertical-scan starts.
    pub vscan_start_us: u64,
    /// Diagnostic subtotal for the 78 synchronous 600-row source scans.
    pub source_scan_us: u64,
    /// Diagnostic subtotal for binary-full inter-pass delays.
    pub pass_delay_us: u64,
    pub finalization_us: u64,
    pub previous_copy_us: u64,
    pub total_us: u64,
}

impl FullRefreshTiming {
    pub const fn waveform_us(self) -> u64 {
        self.initial_clean_us
            .saturating_add(self.framebuffer_us)
            .saturating_add(self.settle_us)
            .saturating_add(self.final_clean_us)
            .saturating_add(self.terminal_vscan_us)
    }
}

impl PartialGateDrainTiming {
    // Constructed only by the hardware-gated partial-refresh gate-drain path;
    // dead on a host build with that module excluded.
    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    const fn full_fallback() -> Self {
        Self {
            scan_rows: E_INK_HEIGHT,
            first_changed_row: 0,
            last_changed_row: E_INK_HEIGHT - 1,
            changed_span_rows: E_INK_HEIGHT,
            source_skip_candidate_rows: 0,
            row_discovery_us: 0,
            transition_prepare_us: 0,
            power_on_us: 0,
            transition_us: 0,
            vscan_start_us: 0,
            source_scan_us: 0,
            pass_delay_us: 0,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us: 0,
            finalization_us: 0,
            previous_copy_us: 0,
            full_fallback: true,
        }
    }

    #[cfg_attr(not(target_os = "none"), allow(dead_code))]
    const fn no_change() -> Self {
        Self {
            scan_rows: 0,
            first_changed_row: 0,
            last_changed_row: 0,
            changed_span_rows: 0,
            source_skip_candidate_rows: 0,
            row_discovery_us: 0,
            transition_prepare_us: 0,
            power_on_us: 0,
            transition_us: 0,
            vscan_start_us: 0,
            source_scan_us: 0,
            pass_delay_us: 0,
            cleanup_zero_us: 0,
            cleanup_neutral_finalize_us: 0,
            finalization_us: 0,
            previous_copy_us: 0,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BinaryFramebufferDebugSnapshot {
    pub current_hash: u32,
    pub previous_hash: Option<u32>,
    pub changed_bytes: usize,
    pub changed_pixels: u32,
    pub min_row: Option<usize>,
    pub max_row: Option<usize>,
    pub min_byte_column: Option<usize>,
    pub max_byte_column: Option<usize>,
}

pub type Result<T, E> = core::result::Result<T, InkplateHalError<E>>;

#[cfg(target_os = "none")]
pub struct InkplateHal<I2C: 'static, D> {
    i2c: I2C,
    delay: D,
    /// Shared with the battery provider task (typed observation
    /// subscriptions plan, Phase 5): see `expander::PcalExpander`'s module
    /// doc for why this moved out of a private field here.
    expander: &'static embassy_sync::mutex::Mutex<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        expander::PcalExpander<I2C>,
    >,
    battery_gate_active_high: Option<bool>,
    pin_lut: [u32; 256],
    panel_fast_ready: bool,
    panel_power_state: panel_lifecycle::PanelPowerState,
    framebuffer_bw: &'static mut [u8; FRAMEBUFFER_BYTES],
    /// Previous frame, used only to diff against `framebuffer_bw` when building
    /// the partial transition. Never read by an interrupt-masked scan pass, so
    /// unlike `framebuffer_bw` it can live in PSRAM; see
    /// docs/references/memory/meditamer-inkplate/budget.md.
    framebuffer_bw_previous: Option<&'static mut [u8; FRAMEBUFFER_BYTES]>,
    partial_transition: Option<&'static mut [u8; PARTIAL_TRANSITION_BYTES]>,
    partial_ready: bool,
}

#[cfg(target_os = "none")]
impl<I2C, D> Drop for InkplateHal<I2C, D> {
    fn drop(&mut self) {
        FRAMEBUFFER_TAKEN.store(false, Ordering::Release);
    }
}

/// `board::Panel`'s geometry/L8-ingestion half of the seam (Phase 1). Refresh
/// stays off this trait and off `InkplateHal` generally: the product drives
/// it through the native `display_bw*_async` API instead, because it is
/// asynchronous I²C-sequenced waveform work that a synchronous trait method
/// cannot express.
#[cfg(target_os = "none")]
impl<I2C, D> board::Panel for InkplateHal<I2C, D> {
    fn geometry(&self) -> board::Geometry {
        board::Geometry {
            width: E_INK_WIDTH as u16,
            height: E_INK_HEIGHT as u16,
        }
    }

    fn blit_l8(&mut self, area: board::DirtyArea, pixels: &[u8]) -> bool {
        panel_blit::blit_l8(area, pixels, self.framebuffer_bw)
    }
}

pub mod adapters;
#[cfg(target_os = "none")]
mod base;
/// Inkplate 4 TEMPERA buzzer driver: nominal pitch mapping plus the retried
/// transport and note-sequencing policy. Host-compatible by design so the
/// host recorder imports the same helper the firmware executes.
pub mod buzzer;
#[cfg(target_os = "none")]
pub mod buzzer_device;
#[cfg(target_os = "none")]
mod clean;
#[cfg(target_os = "none")]
pub use clean::scan_masked_max_us;
#[cfg(target_os = "none")]
mod control;
#[cfg(target_os = "none")]
mod display;
pub mod expander;
#[cfg(target_os = "none")]
mod gpio_fast;
#[cfg(target_os = "none")]
mod hardware;
#[cfg(target_os = "none")]
mod i2c;
#[cfg(target_os = "none")]
pub mod imu;
#[cfg(target_os = "none")]
mod panel;
mod panel_lifecycle;
mod partial_transition;
pub mod touch;
#[cfg(target_os = "none")]
mod waveform;
#[cfg(target_os = "none")]
pub use waveform::{
    PANEL_FULL_CLEAN_HARDWARE_LOOP, PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES,
    PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE, PANEL_FULL_CL_HIGH_HOLD_CYCLES,
    PANEL_FULL_FIXED_HOLD_NOPS, PANEL_FULL_FIXED_HOLD_SELECTED, PANEL_FULL_HARDWARE_LOOP,
    PANEL_FULL_INTER_PASS_DELAY_US, PANEL_FULL_LEGACY_CLEAN_LOOP, PANEL_FULL_OPTIMIZED_CLEAN_LOOP,
    PANEL_FULL_PIPELINED_HOLD, PANEL_FULL_REFERENCE_ROW_BOUNDARY, PANEL_FULL_REFERENCE_SEQUENCE,
    PANEL_FULL_ROW_START_REFERENCE_SEQUENCE, PANEL_FULL_ROW_START_REGISTER_CCOUNT,
    PANEL_FULL_SCAN_PHASE_TIMING, PANEL_FULL_SOURCE_FIXED_HOLD_NOPS,
    PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED, PANEL_GRAYSCALE_CL_HIGH_HOLD_CYCLES,
    PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY, PANEL_GRAYSCALE_REFERENCE_SEQUENCE,
    PANEL_PARTIAL_BOUNDED_TRANSITION_PREP, PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES,
    PANEL_PARTIAL_FIXED_HOLD_NOPS, PANEL_PARTIAL_HARDWARE_LOOP, PANEL_PARTIAL_HOISTED_GPIO_LOOP,
    PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY, PANEL_PARTIAL_REFERENCE_SEQUENCE,
    PANEL_PARTIAL_SCAN_PHASE_TIMING,
};

#[cfg(target_os = "none")]
use hardware::*;

pub mod bus_proxy;

// Embassy's host clock is global: deadline tests and driver retry tests must
// not advance each other's time while the test runner executes in parallel.
#[cfg(test)]
extern crate std;
#[cfg(test)]
static TEST_CLOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
