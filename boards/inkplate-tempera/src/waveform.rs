use super::{
    DelayOps, GpioFast, I2cOps, InkplateHal, Result, CKV_MASK1, CL_MASK, DATA_MASK, IO_INT_ADDR,
    LE_MASK, SPH_MASK1, SPV,
};

// Production partial refresh uses the qualified vendor-style ordered GPIO
// set/clear sequence. Numeric CCOUNT holds remain explicit reproduction and
// boundary-testing selectors.
pub const PANEL_PARTIAL_CCOUNT_HOLD_SELECTED: bool = option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_6")
    .is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_9").is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_10").is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_11").is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_12").is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_24").is_some()
    || option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_48").is_some();
pub const PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES: u32 =
    if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_6").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_9").is_some() {
        9
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_10").is_some() {
        10
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_11").is_some() {
        11
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_12").is_some() {
        12
    } else if option_env!("MEDITAMER_PANEL_CL_HIGH_HOLD_24").is_some() {
        24
    } else {
        48
    };

pub const PANEL_PARTIAL_FIXED_HOLD_SELECTED: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_4_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_6_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_7_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_8_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_11_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_12_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_13_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_14_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_15_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_16_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_18_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_19_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_20_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_24_NOPS").is_some();

// This is a source-level contract rather than a zero-cycle delay call, whose
// compiler output could still contain latency. Explicit hold selectors take
// the production path back to their respective diagnostic implementation.
pub const PANEL_PARTIAL_REFERENCE_SEQUENCE: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_REFERENCE_SEQUENCE").is_some()
        || (!PANEL_PARTIAL_CCOUNT_HOLD_SELECTED && !PANEL_PARTIAL_FIXED_HOLD_SELECTED);

// Production uses the qualified reference CKV-low, LE-high, LE-low boundary.
// The former one-microsecond margin remains selectable for reproduction.
pub const PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_MARGIN_1US").is_none();

// Production hoists the GPIO register values from the fixed source loop and
// uses Xtensa's zero-overhead loop. Explicit legacy selectors retain the two
// earlier compiler shapes for reproduction.
pub const PANEL_PARTIAL_HOISTED_GPIO_LOOP: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_HOISTED_GPIO_LOOP").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_HARDWARE_LOOP").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_LEGACY_GPIO_LOOP").is_none();

pub const PANEL_PARTIAL_HARDWARE_LOOP: bool = PANEL_PARTIAL_HOISTED_GPIO_LOOP
    && (option_env!("MEDITAMER_PANEL_PARTIAL_HARDWARE_LOOP").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_HOISTED_ONLY").is_none());

// Separates pass setup, source scan, and the 230 us inter-pass delay in the
// refresh probe. Keep it opt-in because the extra clock reads are measurement
// overhead, not production work.
pub const PANEL_PARTIAL_SCAN_PHASE_TIMING: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_SCAN_PHASE_TIMING").is_some();

// Production prepares only the transition suffix consumed by the selected
// reverse row scan. The full-frame preparer remains available for reproduction.
pub const PANEL_PARTIAL_BOUNDED_TRANSITION_PREP: bool =
    option_env!("MEDITAMER_PANEL_PARTIAL_BOUNDED_TRANSITION_PREP").is_some()
        || option_env!("MEDITAMER_PANEL_PARTIAL_FULL_TRANSITION_PREP").is_none();

// Symptom this fixes: dismissing Meditamer's Settings overlay left a thin
// vertical line of stale black pixels sitting exactly on the panel's own
// left border after the partial refresh, even though a full refresh of the
// same (already-correct) framebuffer cleared it -- proof the software model
// was right and only the partial-refresh drive missed that column.
//
// Root cause: the reverse row scan's neutral gate-drain segment ends -- and
// the real source-scanned segment begins -- exactly at `first_changed_row`.
// The framebuffer is column-major, so one "row" here is one UI x-column; a
// change whose own leading edge sits at that row (a modal's left border is
// exactly one such row) puts the actual content right on the real/neutral
// scan-mode seam, the same row-band collapse mechanism already recorded in
// docs/references/display-refresh.md from a 2026-09-01 device session.
// Scanning a few extra rows below `first_changed_row` with real data moves
// that seam into rows already known unchanged, clearing the seam artifact.
//
// The default below (8 rows) reproduced clean on a physical Settings
// open/close cycle. It is a soak candidate, not yet bisected to the minimum
// that clears it -- the alternates let a future pass tighten it.
pub const PANEL_PARTIAL_ROW_SPAN_GUARD_ROWS: usize =
    if option_env!("MEDITAMER_PANEL_PARTIAL_ROW_SPAN_GUARD_0").is_some() {
        0
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_ROW_SPAN_GUARD_16").is_some() {
        16
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_ROW_SPAN_GUARD_24").is_some() {
        24
    } else {
        8
    };

// Fixed-instruction partial hold candidates. These values are bounded soak
// candidates, not production defaults; the passing minimum is established by
// visual bisection on the physical panel.
pub const PANEL_PARTIAL_FIXED_HOLD_NOPS: u32 =
    if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_4_NOPS").is_some() {
        4
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_6_NOPS").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_7_NOPS").is_some() {
        7
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_8_NOPS").is_some() {
        8
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_11_NOPS").is_some() {
        11
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_12_NOPS").is_some() {
        12
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_13_NOPS").is_some() {
        13
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_14_NOPS").is_some() {
        14
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_15_NOPS").is_some() {
        15
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_16_NOPS").is_some() {
        16
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_18_NOPS").is_some() {
        18
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_19_NOPS").is_some() {
        19
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_20_NOPS").is_some() {
        20
    } else if option_env!("MEDITAMER_PANEL_PARTIAL_FIXED_HOLD_24_NOPS").is_some() {
        24
    } else {
        0
    };

// Binary full-refresh CL-high threshold experiment. Twelve cycles was clean in
// the first reciprocal A/B while no hold failed after an identical Clock
// partial predecessor. Shorter values remain opt-in until the physical
// partial-to-full regression gate establishes the minimum reliable hold.
pub const PANEL_FULL_CL_HIGH_HOLD_CYCLES: u32 =
    if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_0").is_some() {
        0
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_1").is_some() {
        1
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_2").is_some() {
        2
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_3").is_some() {
        3
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_4").is_some() {
        4
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_5").is_some() {
        5
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_6").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_7").is_some() {
        7
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_8").is_some() {
        8
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_9").is_some() {
        9
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_10").is_some() {
        10
    } else if option_env!("MEDITAMER_PANEL_FULL_CL_HIGH_HOLD_11").is_some() {
        11
    } else {
        12
    };

// Fixed-instruction full-refresh hold candidates. As with the partial path,
// these are qualification inputs rather than production defaults.
pub const PANEL_FULL_FIXED_HOLD_SELECTED: bool =
    option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_0_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_4_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_6_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_7_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_8_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_12_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_14_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_15_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_16_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_20_NOPS").is_some()
        || option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_64_NOPS").is_some();
pub const PANEL_FULL_FIXED_HOLD_NOPS: u32 =
    if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_0_NOPS").is_some() {
        0
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_4_NOPS").is_some() {
        4
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_6_NOPS").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_7_NOPS").is_some() {
        7
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_8_NOPS").is_some() {
        8
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_12_NOPS").is_some() {
        12
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_14_NOPS").is_some() {
        14
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_15_NOPS").is_some() {
        15
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_16_NOPS").is_some() {
        16
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_20_NOPS").is_some() {
        20
    } else if option_env!("MEDITAMER_PANEL_FULL_FIXED_HOLD_64_NOPS").is_some() {
        64
    } else {
        0
    };

// Diagnostic-only reproduction of the failing ordered set/clear sequence.
pub const PANEL_FULL_REFERENCE_SEQUENCE: bool =
    option_env!("MEDITAMER_PANEL_FULL_REFERENCE_SEQUENCE").is_some();

// Diagnostic replay of the pre-qualification clean-pass implementation. The
// production path keeps the GPIO W1TS/W1TC addresses invariant across the 148
// constant-data pulses and emits their ordered set/clear writes inline.
pub const PANEL_FULL_LEGACY_CLEAN_LOOP: bool =
    option_env!("MEDITAMER_PANEL_FULL_LEGACY_CLEAN_LOOP").is_some();
pub const PANEL_FULL_OPTIMIZED_CLEAN_LOOP: bool = !PANEL_FULL_LEGACY_CLEAN_LOOP
    && !PANEL_FULL_FIXED_HOLD_SELECTED
    && !PANEL_FULL_REFERENCE_SEQUENCE;

// Production matches the reference compiler's 74-iteration hardware loop with
// two complete CL pulses per iteration. The earlier software-counter loop stays
// available as an explicit probe-only reproduction shape.
pub const PANEL_FULL_CLEAN_HARDWARE_LOOP: bool =
    option_env!("MEDITAMER_PANEL_FULL_SOFTWARE_CLEAN_LOOP").is_none();

// Production uses immediate ordered GPIO writes for the clean row-start,
// first-data, and final-data pulses. The former CCOUNT edges remain available
// as an explicit probe-only reproduction shape.
pub const PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE: bool =
    option_env!("MEDITAMER_PANEL_FULL_CLEAN_CCOUNT_EDGES").is_none();

// Independent hold selector retained for clean-loop boundary reproduction.
// The approved production default is zero numeric hold for these constant-data
// pulses; all other full-refresh pulses keep PANEL_FULL_CL_HIGH_HOLD_CYCLES.
pub const PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES: u32 =
    if option_env!("MEDITAMER_PANEL_FULL_CLEAN_INLINE_HOLD_6").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_FULL_CLEAN_INLINE_HOLD_9").is_some() {
        9
    } else if option_env!("MEDITAMER_PANEL_FULL_CLEAN_INLINE_HOLD_10").is_some() {
        10
    } else if option_env!("MEDITAMER_PANEL_FULL_CLEAN_INLINE_HOLD_11").is_some() {
        11
    } else if option_env!("MEDITAMER_PANEL_FULL_CLEAN_INLINE_HOLD_12").is_some() {
        12
    } else {
        0
    };

// Enables phase-level full-refresh timings in the stand-alone probe. The
// production API keeps using the unprofiled orchestration instantiation.
pub const PANEL_FULL_SCAN_PHASE_TIMING: bool =
    option_env!("MEDITAMER_PANEL_FULL_SCAN_PHASE_TIMING").is_some();

// The qualified binary-full waveform starts the next pass immediately. The
// former 230 us spacing remains selectable for historical reproduction.
pub const PANEL_FULL_INTER_PASS_DELAY_US: u32 =
    if option_env!("MEDITAMER_PANEL_FULL_INTER_PASS_DELAY_230_US").is_some() {
        230
    } else {
        0
    };

// Production hoists the GPIO/LUT values into one Xtensa zero-overhead source
// loop. The earlier software source loop remains available for reproduction.
pub const PANEL_FULL_HARDWARE_LOOP: bool = option_env!("MEDITAMER_PANEL_FULL_SOFTWARE_SOURCE_LOOP")
    .is_none()
    && !PANEL_FULL_FIXED_HOLD_SELECTED
    && !PANEL_FULL_REFERENCE_SEQUENCE;

// Probe-only scheduling candidate layered on the hardware loop. It performs
// the next LUT lookup while CL is already high, then still checks the same
// 12-cycle CCOUNT threshold before clearing CL.
pub const PANEL_FULL_PIPELINED_HOLD: bool =
    option_env!("MEDITAMER_PANEL_FULL_PIPELINED_HOLD").is_some();

// Probe-only row-start CCOUNT bracket. It forces the threshold through a
// register comparison even at zero so numeric results share one instruction
// shape. This is independent from the older default CCOUNT helper below.
pub const PANEL_FULL_ROW_START_REGISTER_CCOUNT: bool =
    option_env!("MEDITAMER_PANEL_FULL_ROW_START_REGISTER_CCOUNT").is_some();

// Production uses the official immediate row-start sequence for framebuffer
// and settle scans. Both former CCOUNT helper shapes remain probe-selectable.
pub const PANEL_FULL_ROW_START_REFERENCE_SEQUENCE: bool =
    option_env!("MEDITAMER_PANEL_FULL_ROW_START_CCOUNT").is_none()
        && !PANEL_FULL_ROW_START_REGISTER_CCOUNT;

// Production uses the qualified CKV-low, LE-high, LE-low row boundary without
// a numeric delay. The former one-microsecond margin remains probe-selectable.
pub const PANEL_FULL_REFERENCE_ROW_BOUNDARY: bool =
    option_env!("MEDITAMER_PANEL_FULL_MARGIN_1US").is_none();

// Production uses the fixed-zero hardware-loop body for the 150 inner
// framebuffer/settle pulses in each row. Explicit fixed padding values and the
// former CCOUNT body remain available for timing reproduction.
pub const PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED: bool = PANEL_FULL_HARDWARE_LOOP
    && !PANEL_FULL_PIPELINED_HOLD
    && option_env!("MEDITAMER_PANEL_FULL_SOURCE_CCOUNT_HOLD").is_none();
pub const PANEL_FULL_SOURCE_FIXED_HOLD_NOPS: u32 =
    if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_1_NOPS").is_some() {
        1
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_2_NOPS").is_some() {
        2
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_3_NOPS").is_some() {
        3
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_4_NOPS").is_some() {
        4
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_5_NOPS").is_some() {
        5
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_6_NOPS").is_some() {
        6
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_7_NOPS").is_some() {
        7
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_8_NOPS").is_some() {
        8
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_9_NOPS").is_some() {
        9
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_10_NOPS").is_some() {
        10
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_11_NOPS").is_some() {
        11
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_12_NOPS").is_some() {
        12
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_13_NOPS").is_some() {
        13
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_14_NOPS").is_some() {
        14
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_15_NOPS").is_some() {
        15
    } else if option_env!("MEDITAMER_PANEL_FULL_SOURCE_FIXED_HOLD_16_NOPS").is_some() {
        16
    } else {
        0
    };

// Native Gray4 keeps an independent conservative baseline so a grayscale-only
// timing experiment cannot weaken interactive partial refresh.
pub const PANEL_GRAYSCALE_CL_HIGH_HOLD_CYCLES: u32 =
    if option_env!("MEDITAMER_PANEL_GRAYSCALE_CL_HIGH_HOLD_12").is_some() {
        12
    } else {
        48
    };

// Diagnostic-only reference-rate experiment. This selects the official
// driver's source-level contract: one ordered W1TS write immediately followed
// by one ordered W1TC write, with no numeric delay between them. The compiled
// scan bodies are checked separately so this is an instruction-sequence
// contract rather than a zero-cycle magic number.
pub const PANEL_GRAYSCALE_REFERENCE_SEQUENCE: bool =
    option_env!("MEDITAMER_PANEL_GRAYSCALE_REFERENCE_SEQUENCE").is_some();

// Diagnostic-only row-boundary experiment. The official driver ends each row
// with ordered CKV-low, LE-high, and LE-low writes followed by a nominal
// zero-duration delay call. This selects the source-level GPIO ordering without
// turning incidental call latency into a guessed timing constant.
pub const PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY: bool =
    option_env!("MEDITAMER_PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY").is_some();

#[inline(always)]
fn hold_panel_clock_high() {
    if PANEL_PARTIAL_FIXED_HOLD_NOPS != 0 {
        // Keep this as one explicit instruction run. The release-ELF gate
        // verifies its length and rejects a retained CCOUNT polling loop.
        unsafe {
            core::arch::asm!(
                ".rept {hold_nops}",
                "nop",
                ".endr",
                hold_nops = const PANEL_PARTIAL_FIXED_HOLD_NOPS,
                options(nostack)
            );
        }
    } else if !PANEL_PARTIAL_REFERENCE_SEQUENCE {
        esp_hal::xtensa_lx::timer::delay(PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES);
    }
}

// Keep the full-refresh scan loops within Xtensa's short-branch range. The
// release-ELF gate verifies this routine remains in IRAM and is called by each
// full scan body.
#[inline(never)]
#[unsafe(link_section = ".rwtext")]
fn hold_full_refresh_panel_clock_high() {
    if PANEL_FULL_FIXED_HOLD_SELECTED {
        unsafe {
            core::arch::asm!(
                ".rept {hold_nops}",
                "nop",
                ".endr",
                hold_nops = const PANEL_FULL_FIXED_HOLD_NOPS,
                options(nostack)
            );
        }
    } else if PANEL_FULL_PIPELINED_HOLD || PANEL_FULL_ROW_START_REGISTER_CCOUNT {
        let hold_cycles = PANEL_FULL_CL_HIGH_HOLD_CYCLES;
        // Keep the row-start pulse in the same register-comparison bracket as
        // the pipelined inner pulses, including at a zero threshold.
        unsafe {
            core::arch::asm!(
                "rsr.ccount {started}",
                "2:",
                "rsr.ccount {elapsed}",
                "sub {elapsed}, {elapsed}, {started}",
                "bltu {elapsed}, {hold_cycles}, 2b",
                started = out(reg) _,
                elapsed = out(reg) _,
                hold_cycles = in(reg) hold_cycles,
                options(nostack)
            );
        }
    } else if !PANEL_FULL_REFERENCE_SEQUENCE {
        esp_hal::xtensa_lx::timer::delay(PANEL_FULL_CL_HIGH_HOLD_CYCLES);
    }
}

#[inline(always)]
fn hold_grayscale_panel_clock_high() {
    if !PANEL_GRAYSCALE_REFERENCE_SEQUENCE {
        esp_hal::xtensa_lx::timer::delay(PANEL_GRAYSCALE_CL_HIGH_HOLD_CYCLES);
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub(super) async fn vscan_start(&mut self) -> Result<(), I2C::Error> {
        self.set_ckv(true);
        self.delay.delay_us(7);
        self.digital_write_internal(IO_INT_ADDR, SPV, false).await?;
        self.delay.delay_us(10);
        self.set_ckv(false);
        self.delay.delay_us(1);
        self.set_ckv(true);
        self.delay.delay_us(8);
        self.digital_write_internal(IO_INT_ADDR, SPV, true).await?;
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

    #[inline(always)]
    pub(super) fn vscan_end(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
        // Soldered's reference driver calls the ESP ROM delay primitive here
        // with a nominal zero duration. That call-latency-only boundary proved
        // code-layout-sensitive in the Rust release build, so retain an
        // explicit 1 us setup margin before the next SPH/CKV sequence.
        if !PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY {
            esp_hal::rom::ets_delay_us(1);
        }
    }

    #[inline(always)]
    pub(super) fn vscan_end_full_refresh(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
        if !PANEL_FULL_REFERENCE_ROW_BOUNDARY {
            esp_hal::rom::ets_delay_us(1);
        }
    }

    #[inline(always)]
    pub(super) fn vscan_end_grayscale(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
        if !PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY {
            esp_hal::rom::ets_delay_us(1);
        }
    }

    #[inline(always)]
    pub(super) fn hscan_start(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    pub(super) fn hscan_start_full_refresh(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock_full_refresh(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    pub(super) fn hscan_start_full_refresh_reference(&self, d: u32) {
        self.set_sph(false);
        GpioFast::out_set(d | CL_MASK);
        GpioFast::out_clear(DATA_MASK | CL_MASK);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    pub(super) fn hscan_start_grayscale(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock_grayscale(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_full_refresh(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_full_refresh_panel_clock_high();
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_grayscale(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_grayscale_panel_clock_high();
        GpioFast::out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_preserve_data(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_preserve_data_full_refresh(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_full_refresh_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn write_data_and_clock_preserve_data_grayscale(&self, data_word: u32) {
        GpioFast::out_set(data_word | CL_MASK);
        hold_grayscale_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn pulse_cl_only(&self) {
        GpioFast::out_set(CL_MASK);
        hold_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn pulse_cl_only_full_refresh(&self) {
        GpioFast::out_set(CL_MASK);
        hold_full_refresh_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn pulse_cl_only_grayscale(&self) {
        GpioFast::out_set(CL_MASK);
        hold_grayscale_panel_clock_high();
        GpioFast::out_clear(CL_MASK);
    }

    #[inline(always)]
    pub(super) fn clear_data_and_cl_le(&self) {
        GpioFast::out_clear(DATA_MASK | LE_MASK | CL_MASK);
    }

    #[inline(always)]
    pub(super) fn set_le(&self, high: bool) {
        if high {
            GpioFast::out_set(LE_MASK);
        } else {
            GpioFast::out_clear(LE_MASK);
        }
    }

    #[inline(always)]
    pub(super) fn set_cl(&self, high: bool) {
        if high {
            GpioFast::out_set(CL_MASK);
        } else {
            GpioFast::out_clear(CL_MASK);
        }
    }

    #[inline(always)]
    pub(super) fn set_ckv(&self, high: bool) {
        if high {
            GpioFast::out1_set(CKV_MASK1);
        } else {
            GpioFast::out1_clear(CKV_MASK1);
        }
    }

    #[inline(always)]
    pub(super) fn set_sph(&self, high: bool) {
        if high {
            GpioFast::out1_set(SPH_MASK1);
        } else {
            GpioFast::out1_clear(SPH_MASK1);
        }
    }
}
