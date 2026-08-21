use core::mem::MaybeUninit;
use core::sync::atomic::AtomicBool;

use super::FRAMEBUFFER_BYTES;

pub(super) const IO_INT_ADDR: u8 = 0x20;
pub(super) const IO_EXT_ADDR: u8 = 0x21;
pub(super) const TPS65186_ADDR: u8 = 0x48;
pub(super) const FRONTLIGHT_DIGIPOT_ADDR: u8 = 0x2E;
pub(super) const BUZZER_DIGIPOT_ADDR: u8 = 0x2F;
pub(super) const FUEL_GAUGE_ADDR: u8 = 0x55;
pub(super) const LSM6DS3_ADDR: u8 = 0x6B;
pub(super) const TOUCHSCREEN_ADDR: u8 = 0x15;
pub(super) const PWR_GOOD_OK: u8 = 0b1111_1010;
pub(super) const BQ27441_COMMAND_SOC: u8 = 0x1C;
pub(super) const LSM6DS3_REG_WHO_AM_I: u8 = 0x0F;
pub(super) const LSM6DS3_REG_CTRL1_XL: u8 = 0x10;
pub(super) const LSM6DS3_REG_CTRL2_G: u8 = 0x11;
pub(super) const LSM6DS3_REG_TAP_SRC: u8 = 0x1C;
pub(super) const LSM6DS3_REG_OUTX_L_G: u8 = 0x22;
pub(super) const LSM6DS3_REG_TAP_CFG1: u8 = 0x58;
pub(super) const LSM6DS3_REG_TAP_THS_6D: u8 = 0x59;
pub(super) const LSM6DS3_REG_INT_DUR2: u8 = 0x5A;
pub(super) const LSM6DS3_REG_WAKE_UP_THS: u8 = 0x5B;
pub(super) const LSM6DS3_REG_MD1_CFG: u8 = 0x5E;
pub(super) const LSM6DS3_WHO_AM_I_VALUE: u8 = 0x69;

pub(super) const PCAL_REG_ADDRS: [u8; 23] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
    0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4F,
];
pub(super) const PCAL_OUTPORT0_ARRAY: usize = 2;
pub(super) const PCAL_OUTPORT1_ARRAY: usize = 3;
pub(super) const PCAL_CFGPORT0_ARRAY: usize = 6;
pub(super) const PCAL_CFGPORT1_ARRAY: usize = 7;
pub(super) const PCAL_PUPDEN_REG0_ARRAY: usize = 14;
pub(super) const PCAL_PUPDEN_REG1_ARRAY: usize = 15;
pub(super) const PCAL_PUPDSEL_REG0_ARRAY: usize = 16;
pub(super) const PCAL_PUPDSEL_REG1_ARRAY: usize = 17;

pub(super) const OE: u8 = 0;
pub(super) const GMOD: u8 = 1;
pub(super) const SPV: u8 = 2;
pub(super) const WAKEUP: u8 = 3;
pub(super) const PWRUP: u8 = 4;
pub(super) const VCOM: u8 = 5;
pub(super) const GPIO0_ENABLE: u8 = 8;
pub(super) const INT_APDS: u8 = 9;
pub(super) const BATTERY_MEAS_EN: u8 = 9;
pub(super) const FRONTLIGHT_EN: u8 = 10;
pub(super) const SD_PMOS_PIN: u8 = 11;
pub(super) const BUZZ_EN: u8 = 12;
pub(super) const INT2_LSM: u8 = 13;
pub(super) const INT1_LSM: u8 = 14;
pub(super) const FG_GPOUT: u8 = 15;
pub(super) const TOUCHSCREEN_EN: u8 = 0;
pub(super) const TOUCHSCREEN_RST: u8 = 1;

pub(super) const TOUCH_SOFT_RESET_CMD: [u8; 4] = [0x77, 0x77, 0x77, 0x77];
pub(super) const TOUCH_HELLO_PACKET: [u8; 4] = [0x55, 0x55, 0x55, 0x55];
pub(super) const TOUCH_GET_X_RES_CMD: [u8; 4] = [0x53, 0x60, 0x00, 0x00];
pub(super) const TOUCH_GET_Y_RES_CMD: [u8; 4] = [0x53, 0x63, 0x00, 0x00];
pub(super) const TOUCH_SOFT_RESET_POLL_INTERVAL_MS: u32 = 20;
pub(super) const TOUCH_SOFT_RESET_TIMEOUT_MS: u32 = 1_000;

pub(super) const BEEP_FREQ_MIN_HZ: i32 = 572;
pub(super) const BEEP_FREQ_MAX_HZ: i32 = 2933;
pub(super) const ENABLE_BUZZER_PITCH_CONTROL: bool = true;

// Waveform lookup tables are touched inside interrupt-masked IRAM scan loops.
// Keep them in internal DRAM so flash-cache state cannot stretch a row pulse.
#[unsafe(link_section = ".data")]
pub(super) static LUT2: [u8; 16] = [
    0xAA, 0xA9, 0xA6, 0xA5, 0x9A, 0x99, 0x96, 0x95, 0x6A, 0x69, 0x66, 0x65, 0x5A, 0x59, 0x56, 0x55,
];
#[unsafe(link_section = ".data")]
pub(super) static LUTW: [u8; 16] = [
    0xFF, 0xFE, 0xFB, 0xFA, 0xEF, 0xEE, 0xEB, 0xEA, 0xBF, 0xBE, 0xBB, 0xBA, 0xAF, 0xAE, 0xAB, 0xAA,
];
#[unsafe(link_section = ".data")]
pub(super) static LUTB: [u8; 16] = [
    0xFF, 0xFD, 0xF7, 0xF5, 0xDF, 0xDD, 0xD7, 0xD5, 0x7F, 0x7D, 0x77, 0x75, 0x5F, 0x5D, 0x57, 0x55,
];

const GRAYSCALE_WAVEFORM_3BIT: [[u8; 8]; 8] = [
    [0, 0, 1, 1, 1, 1, 1, 0],
    [1, 1, 1, 2, 1, 1, 0, 0],
    [2, 1, 1, 0, 2, 1, 1, 0],
    [0, 0, 0, 1, 1, 1, 2, 0],
    [2, 1, 1, 2, 1, 1, 2, 0],
    [1, 2, 1, 1, 2, 1, 2, 0],
    [1, 1, 1, 2, 1, 2, 2, 0],
    [0, 0, 0, 0, 0, 2, 2, 0],
];

const fn build_grayscale_waveform_lut() -> [[u8; 256]; 8] {
    let mut lut = [[0u8; 256]; 8];
    let mut phase = 0;
    while phase < 8 {
        let mut packed = 0;
        while packed < 256 {
            let low4 = packed & 0x0f;
            let high4 = (packed >> 4) & 0x0f;
            let low3 = (low4 * 7 + 7) / 15;
            let high3 = (high4 * 7 + 7) / 15;
            lut[phase][packed] =
                (GRAYSCALE_WAVEFORM_3BIT[low3][phase] << 2) | GRAYSCALE_WAVEFORM_3BIT[high3][phase];
            packed += 1;
        }
        phase += 1;
    }
    lut
}

// The scan loop runs with interrupts masked, so keep its lookup table in
// internal DRAM instead of flash-backed storage.
#[unsafe(link_section = ".data")]
pub(super) static GRAYSCALE_WAVEFORM_LUT: [[u8; 256]; 8] = build_grayscale_waveform_lut();

pub(super) static FRAMEBUFFER_TAKEN: AtomicBool = AtomicBool::new(false);
// Read by the interrupt-masked scan passes, so this one must stay in internal
// RAM. Its previous-frame counterpart is supplied by the caller instead, which
// lets it live in PSRAM.
#[unsafe(link_section = ".dram2_uninit.framebuffer")]
pub(super) static mut FRAMEBUFFER_BW: MaybeUninit<[u8; FRAMEBUFFER_BYTES]> = MaybeUninit::uninit();
