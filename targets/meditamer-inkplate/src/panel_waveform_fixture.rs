//! Diagnostic-only panel waveform fixture.
//!
//! The target calls this after board/panel initialization but before spawning
//! product, touch, network, or storage tasks. It keeps the reference waveform
//! isolated from UI policy and leaves the final e-ink image available for
//! physical inspection.

use embassy_time::{Duration, Instant, Timer};
use inkplate_tempera::{
    E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES,
    PANEL_FULL_CL_HIGH_HOLD_CYCLES, PANEL_FULL_REFERENCE_SEQUENCE,
    PANEL_GRAYSCALE_CL_HIGH_HOLD_CYCLES, PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY,
    PANEL_GRAYSCALE_REFERENCE_SEQUENCE, PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES,
};
use meditamer_product::firmware::{
    psram::{self, BufferPlacement},
    types::DisplayContext,
};

const GRAY_ROW_BYTES: usize = E_INK_WIDTH / 2;
const BW_ROW_BYTES: usize = E_INK_WIDTH / 8;
const BAND_HEIGHT: usize = E_INK_HEIGHT / 3;
const CHECKER_TILE: usize = 25;
const BINARY_WAVEFORM_FIXTURE: bool =
    option_env!("MEDITAMER_PANEL_BINARY_WAVEFORM_FIXTURE").is_some();
const BINARY_EXPECTED_PATTERN_HASH: u32 = 0x778c_f145;

pub(crate) async fn run(context: &mut DisplayContext) -> ! {
    if BINARY_WAVEFORM_FIXTURE {
        run_binary(context).await
    }
    run_grayscale(context).await
}

async fn run_binary(context: &mut DisplayContext) -> ! {
    let pattern_hash = {
        let framebuffer = context.inkplate.framebuffer_bw_mut();
        fill_binary_reference_pattern(framebuffer);
        fnv1a(framebuffer)
    };
    console::println!(
        "PANEL_BW_FIXTURE stage=ready bytes={} placement=InternalDram pattern=split_stripes_checker pattern_hash=0x{:08x} expected_hash=0x{:08x} passes=78 rows=46800 cl_pulses=7066800 full_timing={} full_cl_high_hold_cycles={} full_row_boundary=official_ordered_ckv_le partial_cl_high_hold_cycles={}",
        FRAMEBUFFER_BYTES,
        pattern_hash,
        BINARY_EXPECTED_PATTERN_HASH,
        if PANEL_FULL_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        PANEL_FULL_CL_HIGH_HOLD_CYCLES,
        PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES,
    );
    if pattern_hash != BINARY_EXPECTED_PATTERN_HASH {
        console::println!("PANEL_BW_FIXTURE outcome=failed stage=pattern_hash");
        park().await
    }

    let total_started = Instant::now();
    let power_on_started = Instant::now();
    if let Err(error) = context.inkplate.eink_on_async().await {
        console::println!(
            "PANEL_BW_FIXTURE outcome=failed stage=power_on elapsed_us={} error={:?}",
            power_on_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let power_on_us = power_on_started.elapsed().as_micros();

    let waveform_started = Instant::now();
    if let Err(error) = context.inkplate.display_bw_async(true).await {
        console::println!(
            "PANEL_BW_FIXTURE outcome=failed stage=waveform power_on_us={} waveform_us={} error={:?}",
            power_on_us,
            waveform_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let waveform_us = waveform_started.elapsed().as_micros();

    let power_off_started = Instant::now();
    if let Err(error) = context.inkplate.eink_off_async().await {
        console::println!(
            "PANEL_BW_FIXTURE outcome=failed stage=power_off power_on_us={} waveform_us={} power_off_us={} error={:?}",
            power_on_us,
            waveform_us,
            power_off_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let power_off_us = power_off_started.elapsed().as_micros();

    console::println!(
        "PANEL_BW_FIXTURE outcome=passed power_on_us={} waveform_us={} power_off_us={} total_us={} pattern_hash=0x{:08x}",
        power_on_us,
        waveform_us,
        power_off_us,
        total_started.elapsed().as_micros(),
        pattern_hash
    );
    park().await
}

async fn run_grayscale(context: &mut DisplayContext) -> ! {
    let buffer = match psram::alloc_large_byte_buffer(GRAYSCALE_FRAMEBUFFER_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            console::println!(
                "PANEL_GRAY4_FIXTURE outcome=failed stage=allocate error={:?}",
                error
            );
            park().await
        }
    };
    let placement = buffer.placement();
    if !matches!(placement, BufferPlacement::Psram) {
        console::println!(
            "PANEL_GRAY4_FIXTURE outcome=failed stage=placement expected=Psram actual={:?}",
            placement
        );
        park().await
    }

    let framebuffer = buffer.into_static_mut_slice();
    fill_reference_pattern(framebuffer);
    let pattern_hash = fnv1a(framebuffer);
    console::println!(
        "PANEL_GRAY4_FIXTURE stage=ready bytes={} placement={:?} pattern=levels_ramp_boundaries pattern_hash=0x{:08x} timing={} row_boundary={} configured_cl_high_hold_cycles={}",
        framebuffer.len(),
        placement,
        pattern_hash,
        if PANEL_GRAYSCALE_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        if PANEL_GRAYSCALE_REFERENCE_ROW_BOUNDARY {
            "official_ordered_ckv_le"
        } else {
            "ordered_ckv_le_plus_1us"
        },
        PANEL_GRAYSCALE_CL_HIGH_HOLD_CYCLES
    );

    let total_started = Instant::now();
    let power_on_started = Instant::now();
    if let Err(error) = context.inkplate.eink_on_async().await {
        console::println!(
            "PANEL_GRAY4_FIXTURE outcome=failed stage=power_on elapsed_us={} error={:?}",
            power_on_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let power_on_us = power_on_started.elapsed().as_micros();

    let waveform_started = Instant::now();
    if let Err(error) = context
        .inkplate
        .display_gray4_async(framebuffer, true)
        .await
    {
        console::println!(
            "PANEL_GRAY4_FIXTURE outcome=failed stage=waveform power_on_us={} waveform_us={} error={:?}",
            power_on_us,
            waveform_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let waveform_us = waveform_started.elapsed().as_micros();

    let power_off_started = Instant::now();
    if let Err(error) = context.inkplate.eink_off_async().await {
        console::println!(
            "PANEL_GRAY4_FIXTURE outcome=failed stage=power_off power_on_us={} waveform_us={} power_off_us={} error={:?}",
            power_on_us,
            waveform_us,
            power_off_started.elapsed().as_micros(),
            error
        );
        park().await
    }
    let power_off_us = power_off_started.elapsed().as_micros();

    console::println!(
        "PANEL_GRAY4_FIXTURE outcome=passed power_on_us={} waveform_us={} power_off_us={} total_us={} pattern_hash=0x{:08x}",
        power_on_us,
        waveform_us,
        power_off_us,
        total_started.elapsed().as_micros(),
        pattern_hash
    );
    park().await
}

fn fill_reference_pattern(framebuffer: &mut [u8]) {
    assert_eq!(framebuffer.len(), GRAYSCALE_FRAMEBUFFER_BYTES);
    for (index, packed) in framebuffer.iter_mut().enumerate() {
        let y = index / GRAY_ROW_BYTES;
        let x = (index % GRAY_ROW_BYTES) * 2;
        let high = gray4_at(x, y);
        let low = gray4_at(x + 1, y);
        *packed = (high << 4) | low;
    }
}

fn fill_binary_reference_pattern(framebuffer: &mut [u8]) {
    assert_eq!(framebuffer.len(), FRAMEBUFFER_BYTES);
    for (index, packed) in framebuffer.iter_mut().enumerate() {
        let y = index / BW_ROW_BYTES;
        let x = (index % BW_ROW_BYTES) * 8;
        let mut value = 0u8;
        for bit in 0..8 {
            if binary_at(x + bit, y) {
                value |= 1 << bit;
            }
        }
        *packed = value;
    }
}

fn binary_at(x: usize, y: usize) -> bool {
    if y < BAND_HEIGHT {
        return x < E_INK_WIDTH / 2;
    }
    if y < BAND_HEIGHT * 2 {
        return (x / CHECKER_TILE) & 1 == 0;
    }
    ((x / CHECKER_TILE) + ((y - BAND_HEIGHT * 2) / CHECKER_TILE)) & 1 == 0
}

fn gray4_at(x: usize, y: usize) -> u8 {
    if y < BAND_HEIGHT {
        let physical_level = (x * 8 / E_INK_WIDTH).min(7);
        return (physical_level * 2) as u8;
    }
    if y < BAND_HEIGHT * 2 {
        return (x * 15 / (E_INK_WIDTH - 1)) as u8;
    }

    let checker = ((x / CHECKER_TILE) + ((y - BAND_HEIGHT * 2) / CHECKER_TILE)) & 1;
    if x < E_INK_WIDTH / 2 {
        if checker == 0 {
            0
        } else {
            15
        }
    } else if checker == 0 {
        6
    } else {
        8
    }
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}
