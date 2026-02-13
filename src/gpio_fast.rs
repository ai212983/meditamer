/// GPIO bitmask for e-paper data bus D0..D7
/// (GPIO4/5/18/19/23/25/26/27 on Inkplate 4 TEMPERA).
pub const DATA_MASK: u32 = 0x0E8C_0030;
/// GPIO0 clock line (CL) bit in `GPIO.out_*`.
pub const CL_MASK: u32 = 1 << 0;
/// GPIO2 latch enable line (LE) bit in `GPIO.out_*`.
pub const LE_MASK: u32 = 1 << 2;
/// GPIO32 CKV bit in `GPIO.out1_*`.
pub const CKV_MASK1: u32 = 1 << 0;
/// GPIO33 SPH bit in `GPIO.out1_*`.
pub const SPH_MASK1: u32 = 1 << 1;
/// Output-enable mask for bank0 e-paper fast pins.
pub const PANEL_OUT_ENABLE_MASK: u32 = DATA_MASK | CL_MASK | LE_MASK;
/// Output-enable mask for bank1 e-paper fast pins.
pub const PANEL_OUT1_ENABLE_MASK: u32 = CKV_MASK1 | SPH_MASK1;

/// Minimal fast-write helpers used by the upcoming display waveform port.
pub struct GpioFast;

impl GpioFast {
    #[inline(always)]
    pub fn out_set(mask: u32) {
        // Write-1-to-set register, same semantics as ESP-IDF fast path.
        unsafe {
            (*esp32::GPIO::PTR).out_w1ts().write(|w| w.bits(mask));
        }
    }

    #[inline(always)]
    pub fn out_clear(mask: u32) {
        // Write-1-to-clear register, same semantics as ESP-IDF fast path.
        unsafe {
            (*esp32::GPIO::PTR).out_w1tc().write(|w| w.bits(mask));
        }
    }

    #[inline(always)]
    pub fn out1_set(mask: u32) {
        unsafe {
            (*esp32::GPIO::PTR).out1_w1ts().write(|w| w.bits(mask));
        }
    }

    #[inline(always)]
    pub fn out1_clear(mask: u32) {
        unsafe {
            (*esp32::GPIO::PTR).out1_w1tc().write(|w| w.bits(mask));
        }
    }

    #[inline(always)]
    pub fn out_enable_set(mask: u32) {
        unsafe {
            (*esp32::GPIO::PTR).enable_w1ts().write(|w| w.bits(mask));
        }
    }

    #[inline(always)]
    pub fn out_enable1_set(mask: u32) {
        unsafe {
            (*esp32::GPIO::PTR).enable1_w1ts().write(|w| w.bits(mask));
        }
    }
}
