pub(crate) use crate::esp_radio::legacy_preempt_builtin::timer::setup_timebase as setup_timer;

// ESP32 uses Software1 (priority 3) for task switching, because it reserves
// Software0 for the Bluetooth stack.
const SW_INTERRUPT: u32 = if cfg!(esp32) { 1 << 29 } else { 1 << 7 };

pub(crate) fn setup_multitasking() {
    unsafe {
        let enabled = esp_hal::xtensa_lx::interrupt::disable();
        esp_hal::xtensa_lx::interrupt::enable_mask(
            SW_INTERRUPT
                | esp_hal::xtensa_lx_rt::interrupt::CpuInterruptLevel::Level2.mask()
                | esp_hal::xtensa_lx_rt::interrupt::CpuInterruptLevel::Level6.mask()
                | enabled,
        );
    }
}

pub(crate) fn disable_multitasking() {
    esp_hal::xtensa_lx::interrupt::disable_mask(SW_INTERRUPT);
}

pub(crate) fn yield_task() {
    let intr = SW_INTERRUPT;
    unsafe { core::arch::asm!("wsr.intset  {0}", in(reg) intr, options(nostack)) };
}
