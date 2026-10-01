//! Shared probe resources: I2C device type, PSRAM buffers, fault stop.
//!
//! The soak keeps its framebuffers in PSRAM and its driver plus panel-pin
//! ownership in one external value. Allocation or placement failures halt
//! with a machine-readable event instead of running a degraded soak.

use inkplate_tempera::{
    adapters::BusyDelay, InkplateHal, FRAMEBUFFER_BYTES, PARTIAL_TRANSITION_BYTES,
};
use meditamer_product::firmware::{
    psram::{self, BufferPlacement},
    types::{ConfiguredI2cDevice, PanelPinHold},
};

use super::probe_mode::PANEL_I2C_KHZ;

pub(crate) type ProbeI2cDevice = ConfiguredI2cDevice<PANEL_I2C_KHZ>;

pub(crate) struct SoakResources {
    pub(crate) inkplate: InkplateHal<ProbeI2cDevice, BusyDelay>,
    pub(crate) _panel_pins: PanelPinHold<'static>,
}

pub(crate) fn install_partial_buffers(inkplate: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    let previous = allocate_psram_buffer("previous", FRAMEBUFFER_BYTES);
    if !inkplate.install_previous_framebuffer(previous) {
        console::println!("PANEL_SOAK event=halt stage=install_previous");
        halt()
    }
    let transition = allocate_psram_buffer("transition", PARTIAL_TRANSITION_BYTES);
    if !inkplate.install_partial_transition_buffer(transition) {
        console::println!("PANEL_SOAK event=halt stage=install_transition");
        halt()
    }
}

pub(crate) fn allocate_psram_buffer(label: &str, bytes: usize) -> &'static mut [u8] {
    let buffer = match psram::alloc_large_byte_buffer(bytes) {
        Ok(buffer) => buffer,
        Err(error) => {
            console::println!(
                "PANEL_SOAK event=halt stage=allocate buffer={} bytes={} error={:?}",
                label,
                bytes,
                error
            );
            halt()
        }
    };
    if !matches!(buffer.placement(), BufferPlacement::Psram) {
        console::println!(
            "PANEL_SOAK event=halt stage=placement buffer={} bytes={} actual={:?}",
            label,
            bytes,
            buffer.placement()
        );
        halt()
    }
    buffer.into_static_mut_slice()
}

pub(crate) fn halt() -> ! {
    loop {
        esp_hal::delay::Delay::new().delay_millis(1_000);
    }
}
