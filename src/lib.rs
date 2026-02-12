use anyhow::{anyhow, bail, Context, Result};
use core::{convert::Infallible, ptr};
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Pixel, Size},
};
use esp_idf_sys as sys;
use std::ffi::CStr;

const E_INK_WIDTH: usize = 600;
const E_INK_HEIGHT: usize = 600;
const I2C_SDA_PIN: i32 = 21;
const I2C_SCL_PIN: i32 = 22;
const I2C_FREQ_HZ: u32 = 100_000;
// Never block forever on I2C transactions; some peripherals may be absent.
const I2C_TIMEOUT_MS: i32 = 80;

const IO_INT_ADDR: u8 = 0x20;
const IO_EXT_ADDR: u8 = 0x21;
const TPS65186_ADDR: u8 = 0x48;
const FRONTLIGHT_DIGIPOT_ADDR: u8 = 0x2E; // 0x5C >> 1
const BUZZER_DIGIPOT_ADDR: u8 = 0x2F;
const PWR_GOOD_OK: u8 = 0b1111_1010;
const BEEP_FREQ_MIN_HZ: i32 = 572;
const BEEP_FREQ_MAX_HZ: i32 = 2933;
const ENABLE_BUZZER_PITCH_CONTROL: bool = true;

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

// Internal IO expander pins
const OE: u8 = 0;
const GMOD: u8 = 1;
const SPV: u8 = 2;
const WAKEUP: u8 = 3;
const PWRUP: u8 = 4;
const VCOM: u8 = 5;
const GPIO0_ENABLE: u8 = 8;
const INT_APDS: u8 = 9;
const FRONTLIGHT_EN: u8 = 10;
const SD_PMOS_PIN: u8 = 11;
const BUZZ_EN: u8 = 12;
const INT2_LSM: u8 = 13;
const INT1_LSM: u8 = 14;
const FG_GPOUT: u8 = 15;

// External IO expander pins
const TOUCHSCREEN_EN: u8 = 0;

// ESP32 GPIO fast-write masks
const DATA_MASK: u32 = 0x0E8C_0030; // D0..D7 = GPIO4/5/18/19/23/25/26/27
const CL_MASK: u32 = 1 << 0; // GPIO0
const LE_MASK: u32 = 1 << 2; // GPIO2
const CKV_MASK1: u32 = 1 << 0; // GPIO32 in GPIO.out1_w1ts/w1tc
const SPH_MASK1: u32 = 1 << 1; // GPIO33 in GPIO.out1_w1ts/w1tc

const LUT2: [u8; 16] = [
    0xAA, 0xA9, 0xA6, 0xA5, 0x9A, 0x99, 0x96, 0x95, 0x6A, 0x69, 0x66, 0x65, 0x5A, 0x59, 0x56, 0x55,
];
const LUTB: [u8; 16] = [
    0xFF, 0xFD, 0xF7, 0xF5, 0xDF, 0xDD, 0xD7, 0xD5, 0x7F, 0x7D, 0x77, 0x75, 0x5F, 0x5D, 0x57, 0x55,
];

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

impl Default for TestPattern {
    fn default() -> Self {
        Self::CheckerboardDiagonals
    }
}

#[inline(always)]
fn delay_us(us: u32) {
    unsafe { sys::esp_rom_delay_us(us) }
}

#[inline(always)]
fn delay_ms(ms: u32) {
    delay_us(ms.saturating_mul(1000));
}

fn esp_err_name(err: sys::esp_err_t) -> String {
    let c_name = unsafe { sys::esp_err_to_name(err) };
    if c_name.is_null() {
        return format!("err={err}");
    }

    unsafe { CStr::from_ptr(c_name) }
        .to_string_lossy()
        .into_owned()
}

fn check_esp(err: sys::esp_err_t, op: &str) -> Result<()> {
    if err == sys::ESP_OK {
        return Ok(());
    }

    bail!("{op} failed: {} ({err})", esp_err_name(err));
}

struct I2cMasterBus {
    bus: sys::i2c_master_bus_handle_t,
    devices: [sys::i2c_master_dev_handle_t; 128],
}

impl I2cMasterBus {
    fn new() -> Result<Self> {
        let mut bus = ptr::null_mut();

        let mut bus_config = sys::i2c_master_bus_config_t {
            i2c_port: sys::i2c_port_t_I2C_NUM_0 as sys::i2c_port_num_t,
            sda_io_num: I2C_SDA_PIN as sys::gpio_num_t,
            scl_io_num: I2C_SCL_PIN as sys::gpio_num_t,
            // Use the ESP-IDF default clock source for I2C master bus.
            clk_source: sys::soc_periph_i2c_clk_src_t_I2C_CLK_SRC_DEFAULT,
            glitch_ignore_cnt: 7,
            intr_priority: 0,
            trans_queue_depth: 0,
            ..Default::default()
        };
        bus_config.flags.set_enable_internal_pullup(1);

        let err = unsafe { sys::i2c_new_master_bus(&bus_config as *const _, &mut bus as *mut _) };
        check_esp(err, "i2c_new_master_bus")?;

        Ok(Self {
            bus,
            devices: [ptr::null_mut(); 128],
        })
    }

    fn write(&mut self, addr: u8, bytes: &[u8]) -> Result<()> {
        self.with_recovery(addr, "i2c_master_transmit", |dev| unsafe {
            sys::i2c_master_transmit(dev, bytes.as_ptr(), bytes.len(), I2C_TIMEOUT_MS)
        })
    }

    fn write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<()> {
        self.with_recovery(addr, "i2c_master_transmit_receive", |dev| unsafe {
            sys::i2c_master_transmit_receive(
                dev,
                bytes.as_ptr(),
                bytes.len(),
                buffer.as_mut_ptr(),
                buffer.len(),
                I2C_TIMEOUT_MS,
            )
        })
    }

    fn reset(&mut self) -> Result<()> {
        let err = unsafe { sys::i2c_master_bus_reset(self.bus) };
        check_esp(err, "i2c_master_bus_reset")
    }

    fn probe(&mut self, addr: u8) -> Result<bool> {
        let err = unsafe { sys::i2c_master_probe(self.bus, addr as u16, I2C_TIMEOUT_MS) };
        if err == sys::ESP_OK {
            return Ok(true);
        }
        if err == sys::ESP_ERR_NOT_FOUND as i32 {
            return Ok(false);
        }
        check_esp(err, "i2c_master_probe")?;
        Ok(false)
    }

    fn with_recovery<F>(&mut self, addr: u8, op: &str, mut tx: F) -> Result<()>
    where
        F: FnMut(sys::i2c_master_dev_handle_t) -> sys::esp_err_t,
    {
        let mut last_err = sys::ESP_FAIL as sys::esp_err_t;
        for attempt in 0..2 {
            let dev = self.device_for_addr(addr)?;
            let err = tx(dev);
            if err == sys::ESP_OK {
                return Ok(());
            }

            last_err = err;
            if attempt == 0 {
                // If a device NACKs and the bus transitions to invalid state, reset and
                // recreate the device handle so subsequent traffic can continue.
                self.invalidate_device(addr);
                let _ = self.reset();
                delay_ms(1);
            }
        }

        check_esp(last_err, op)
    }

    fn invalidate_device(&mut self, addr: u8) {
        let idx = addr as usize;
        if idx >= self.devices.len() {
            return;
        }

        let dev = self.devices[idx];
        if !dev.is_null() {
            unsafe { sys::i2c_master_bus_rm_device(dev) };
            self.devices[idx] = ptr::null_mut();
        }
    }

    fn device_for_addr(&mut self, addr: u8) -> Result<sys::i2c_master_dev_handle_t> {
        let idx = addr as usize;
        if idx >= self.devices.len() {
            bail!("invalid i2c address 0x{addr:02X}");
        }

        let existing = self.devices[idx];
        if !existing.is_null() {
            return Ok(existing);
        }

        let device_config = sys::i2c_device_config_t {
            dev_addr_length: sys::i2c_addr_bit_len_t_I2C_ADDR_BIT_LEN_7,
            device_address: addr as u16,
            // Digipot at 0x2F can be sensitive; use slightly slower clock and
            // a non-zero SCL wait to improve ACK reliability.
            scl_speed_hz: if addr == BUZZER_DIGIPOT_ADDR {
                50_000
            } else {
                I2C_FREQ_HZ
            },
            scl_wait_us: if addr == BUZZER_DIGIPOT_ADDR { 2000 } else { 0 },
            ..Default::default()
        };

        let mut dev = ptr::null_mut();
        let err = unsafe {
            sys::i2c_master_bus_add_device(self.bus, &device_config as *const _, &mut dev as *mut _)
        };
        check_esp(err, "i2c_master_bus_add_device")?;
        self.devices[idx] = dev;

        Ok(dev)
    }
}

impl Drop for I2cMasterBus {
    fn drop(&mut self) {
        for dev in &mut self.devices {
            if !dev.is_null() {
                unsafe { sys::i2c_master_bus_rm_device(*dev) };
                *dev = ptr::null_mut();
            }
        }

        if !self.bus.is_null() {
            unsafe { sys::i2c_del_master_bus(self.bus) };
            self.bus = ptr::null_mut();
        }
    }
}

pub struct Inkplate {
    i2c: I2cMasterBus,
    io_regs_int: [u8; 23],
    io_regs_ext: [u8; 23],
    ext_io_present: bool,
    panel_on: bool,
    block_partial: bool,
    pin_lut: [u32; 256],
    framebuffer_bw: Vec<u8>, // Equivalent of _partial in Arduino
    previous_bw: Vec<u8>,    // Equivalent of DMemoryNew in Arduino
}

impl Inkplate {
    pub fn new() -> Result<Self> {
        let i2c_driver = I2cMasterBus::new().context("init I2C bus")?;

        let mut pin_lut = [0u32; 256];
        for (i, slot) in pin_lut.iter_mut().enumerate() {
            let v = i as u8;
            *slot = (((v & 0b0000_0011) as u32) << 4)
                | ((((v & 0b0000_1100) >> 2) as u32) << 18)
                | ((((v & 0b0001_0000) >> 4) as u32) << 23)
                | ((((v & 0b1110_0000) >> 5) as u32) << 25);
        }

        Ok(Self {
            i2c: i2c_driver,
            io_regs_int: [0; 23],
            io_regs_ext: [0; 23],
            ext_io_present: false,
            panel_on: false,
            block_partial: true,
            pin_lut,
            framebuffer_bw: vec![0; E_INK_WIDTH * E_INK_HEIGHT / 8],
            previous_bw: vec![0; E_INK_WIDTH * E_INK_HEIGHT / 8],
        })
    }

    pub fn init(&mut self) -> Result<()> {
        self.io_begin(IO_INT_ADDR)?;
        // Some 4 TEMPERA variants/situations NACK on 0x21 and can leave the I2C
        // driver in a bad state; this app does not require external expander IO.
        self.ext_io_present = false;

        // Initialize TPS control pins on internal expander
        self.pin_mode_internal(IO_INT_ADDR, VCOM, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, PWRUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, WAKEUP, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GPIO0_ENABLE, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, GPIO0_ENABLE, true)?;

        // Initial power sequence setup (same as Arduino begin())
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        delay_ms(1);
        self.i2c_write(
            TPS65186_ADDR,
            &[0x09, 0b0001_1011, 0b0000_0000, 0b0001_1011, 0b0000_0000],
        )?;
        delay_ms(1);
        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;

        // Control/data pins
        self.pins_as_outputs()?;
        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Output)?;

        // Match Arduino board init for internal expander defaults.
        self.pin_mode_internal(IO_INT_ADDR, INT_APDS, PinMode::InputPullUp)?;
        self.pin_mode_internal(IO_INT_ADDR, INT2_LSM, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, INT1_LSM, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, BUZZ_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)?;
        self.pin_mode_internal(IO_INT_ADDR, SD_PMOS_PIN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, SD_PMOS_PIN, true)?;
        self.pin_mode_internal(IO_INT_ADDR, FG_GPOUT, PinMode::InputPullUp)?;

        // Frontlight off by default
        self.pin_mode_internal(IO_INT_ADDR, FRONTLIGHT_EN, PinMode::Output)?;
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)?;

        // Put external expander pins in a known low-power output-low state for this board
        if self.ext_io_present {
            // Disable touchscreen by default (active-low enable).
            self.pin_mode_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, PinMode::Output)?;
            self.digital_write_internal(IO_EXT_ADDR, TOUCHSCREEN_EN, true)?;

            for pin in 2u8..16u8 {
                self.pin_mode_internal(IO_EXT_ADDR, pin, PinMode::Output)?;
                self.digital_write_internal(IO_EXT_ADDR, pin, false)?;
            }
        }

        self.panel_on = false;
        Ok(())
    }

    pub fn width(&self) -> usize {
        E_INK_WIDTH
    }

    pub fn height(&self) -> usize {
        E_INK_HEIGHT
    }

    pub fn clear_bw(&mut self) {
        self.framebuffer_bw.fill(0);
    }

    pub fn set_pixel_bw(&mut self, x: usize, y: usize, black: bool) {
        if x >= E_INK_WIDTH || y >= E_INK_HEIGHT {
            return;
        }

        let byte_idx = (E_INK_WIDTH / 8) * y + (x / 8);
        let bit = 1u8 << (x % 8);
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
                    let black_row = (y / 50) % 2 == 0;
                    if black_row {
                        for x in 0..E_INK_WIDTH {
                            self.set_pixel_bw(x, y, true);
                        }
                    }
                }
            }
            TestPattern::SolidBlack => {
                self.framebuffer_bw.fill(0xFF);
            }
            TestPattern::SolidWhite => {
                self.framebuffer_bw.fill(0x00);
            }
        }
    }

    pub fn set_brightness(&mut self, brightness: u8) -> Result<()> {
        self.frontlight_on()?;
        // Give frontlight rail/switch a moment to settle before talking to digipot.
        delay_ms(2);

        let cmd = [0x00, 63u8.saturating_sub(brightness & 0b0011_1111)];
        let mut last_err = None;

        for _ in 0..3 {
            match self.i2c_write(FRONTLIGHT_DIGIPOT_ADDR, &cmd) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = Some((FRONTLIGHT_DIGIPOT_ADDR, err));
                    delay_ms(2);
                }
            }
        }

        if let Some((addr, err)) = last_err {
            println!(
                "frontlight digipot not responding at 0x{addr:02X}: {err:?}; continuing without brightness control"
            );
        }

        // Do not fail app startup if only brightness control is missing.
        Ok(())
    }

    pub fn frontlight_on(&mut self) -> Result<()> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, true)
    }

    pub fn frontlight_off(&mut self) -> Result<()> {
        self.digital_write_internal(IO_INT_ADDR, FRONTLIGHT_EN, false)
    }

    pub fn play_short_song(&mut self) -> Result<()> {
        // Adapted from the official Inkplate 4 TEMPERA buzzer example.
        // Cmaj7-like motif: C, E, G, B plus a short return phrase.
        const SONG: &[(i32, u32, u32)] = &[
            (523, 100, 220),
            (659, 100, 220),
            (783, 100, 220),
            (987, 140, 300),
            (783, 80, 110),
            (659, 80, 110),
            (523, 120, 0),
        ];

        for &(freq_hz, length_ms, pause_ms) in SONG {
            if let Err(err) = self.beep(length_ms, freq_hz) {
                // Ensure buzzer is not left on if a note fails.
                let _ = self.buzzer_off();
                return Err(err);
            }
            if pause_ms > 0 {
                delay_ms(pause_ms);
            }
        }

        // Ensure buzzer ends in off state.
        self.buzzer_off()
    }

    pub fn play_gentle_notification(&mut self) -> Result<()> {
        // Gentle rise-and-fall notification tuned for this buzzer.
        const GENTLE: &[(i32, u32, u32)] = &[
            (784, 70, 35),
            (988, 85, 45),
            (1175, 110, 180),
            (988, 75, 35),
            (784, 120, 0),
        ];

        for &(freq_hz, length_ms, pause_ms) in GENTLE {
            if let Err(err) = self.beep(length_ms, freq_hz) {
                let _ = self.buzzer_off();
                return Err(err);
            }
            if pause_ms > 0 {
                delay_ms(pause_ms);
            }
        }

        self.buzzer_off()
    }

    pub fn play_bell_chime(&mut self) -> Result<()> {
        self.play_gentle_notification()
    }

    pub fn beep(&mut self, length_ms: u32, freq_hz: i32) -> Result<()> {
        if let Err(err) = self.buzzer_on(freq_hz) {
            // If enabling or tuning fails, force off before returning.
            let _ = self.buzzer_off();
            return Err(err);
        }
        delay_ms(length_ms);
        self.buzzer_off()
    }

    pub fn buzzer_on(&mut self, freq_hz: i32) -> Result<()> {
        // Match Arduino sequence: BUZZ_EN low first, then tune digipot.
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, false)?;
        // Give gated buzzer/digipot rail a moment to stabilize.
        delay_ms(1);
        if let Err(err) = self.set_buzzer_frequency(freq_hz) {
            let _ = self.buzzer_off();
            return Err(err);
        }
        Ok(())
    }

    pub fn buzzer_off(&mut self) -> Result<()> {
        self.digital_write_internal(IO_INT_ADDR, BUZZ_EN, true)
    }

    pub fn display_bw(&mut self, leave_on: bool) -> Result<()> {
        self.previous_bw.copy_from_slice(&self.framebuffer_bw);

        self.eink_on()?;

        self.clean(0, 5)?;
        self.clean(1, 15)?;
        self.clean(0, 15)?;
        self.clean(1, 15)?;
        self.clean(0, 15)?;

        // Main black/white write phases
        for _ in 0..10 {
            let mut ptr = self.previous_bw.len() as isize - 1;
            self.vscan_start()?;

            for _ in 0..E_INK_HEIGHT {
                let dram = self.previous_bw[ptr as usize];
                ptr -= 1;

                let mut data = LUTB[(dram >> 4) as usize];
                let mut send = self.pin_lut[data as usize];
                self.hscan_start(send);

                data = LUTB[(dram & 0x0F) as usize];
                send = self.pin_lut[data as usize];
                self.write_data_and_clock(send);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    let d = self.previous_bw[ptr as usize];
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

            delay_us(230);
        }

        // Final phase
        let mut pos = self.previous_bw.len() as isize - 1;
        self.vscan_start()?;
        for _ in 0..E_INK_HEIGHT {
            let dram = self.previous_bw[pos as usize];
            pos -= 1;

            let mut data = LUT2[(dram >> 4) as usize];
            let mut send = self.pin_lut[data as usize];
            self.hscan_start(send);

            data = LUT2[(dram & 0x0F) as usize];
            send = self.pin_lut[data as usize];
            self.write_data_and_clock(send);

            for _ in 0..(E_INK_WIDTH / 8 - 1) {
                let d = self.previous_bw[pos as usize];
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
        delay_us(230);

        self.clean(2, 1)?;
        self.clean(3, 1)?;
        self.vscan_start()?;

        if !leave_on {
            self.eink_off()?;
        }

        self.block_partial = false;
        Ok(())
    }

    pub fn eink_on(&mut self) -> Result<()> {
        if self.panel_on {
            return Ok(());
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, true)?;
        delay_ms(5);

        // Enable rails
        self.i2c_write(TPS65186_ADDR, &[0x01, 0b0010_0000])?;
        // Modify power-up sequence
        self.i2c_write(TPS65186_ADDR, &[0x09, 0b1110_0100])?;
        // Modify power-down sequence (VEE and VNEG swapped)
        self.i2c_write(TPS65186_ADDR, &[0x0B, 0b0001_1011])?;

        self.pins_as_outputs()?;
        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Output)?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Output)?;

        self.set_le(false);
        self.set_cl(false);
        self.set_sph(true);
        self.digital_write_internal(IO_INT_ADDR, GMOD, true)?;
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        self.set_ckv(false);
        self.digital_write_internal(IO_INT_ADDR, OE, false)?;
        self.digital_write_internal(IO_INT_ADDR, PWRUP, true)?;
        self.panel_on = true;

        let mut ok = false;
        let mut last_pg = 0u8;
        for _ in 0..250 {
            delay_ms(1);
            last_pg = self.read_power_good()?;
            if last_pg == PWR_GOOD_OK {
                ok = true;
                break;
            }
        }

        if !ok {
            eprintln!(
                "TPS65186 power-good timeout, last register value: 0x{last_pg:02X} (expected 0x{PWR_GOOD_OK:02X})"
            );
            self.eink_off()?;
            bail!("TPS65186 power-good timeout");
        }

        self.digital_write_internal(IO_INT_ADDR, VCOM, true)?;
        self.digital_write_internal(IO_INT_ADDR, OE, true)?;

        Ok(())
    }

    pub fn eink_off(&mut self) -> Result<()> {
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
            delay_ms(1);
            if self.read_power_good()? == 0 {
                break;
            }
        }

        self.digital_write_internal(IO_INT_ADDR, WAKEUP, false)?;
        self.i2c_write(TPS65186_ADDR, &[0x01, 0x00])?;
        self.pins_z_state()?;

        self.panel_on = false;
        Ok(())
    }

    pub fn debug_dump(&mut self, tag: &str) -> Result<()> {
        let tps_01 = self.read_i2c_reg(TPS65186_ADDR, 0x01)?;
        let tps_09 = self.read_i2c_reg(TPS65186_ADDR, 0x09)?;
        let tps_0b = self.read_i2c_reg(TPS65186_ADDR, 0x0B)?;
        let tps_0f = self.read_i2c_reg(TPS65186_ADDR, 0x0F)?;
        println!(
            "{tag} TPS65186 regs: R01=0x{tps_01:02X} R09=0x{tps_09:02X} R0B=0x{tps_0b:02X} R0F=0x{tps_0f:02X}"
        );

        let int_out0 = self.read_i2c_reg(IO_INT_ADDR, 0x02)?;
        let int_out1 = self.read_i2c_reg(IO_INT_ADDR, 0x03)?;
        let int_cfg0 = self.read_i2c_reg(IO_INT_ADDR, 0x06)?;
        let int_cfg1 = self.read_i2c_reg(IO_INT_ADDR, 0x07)?;
        println!(
            "{tag} PCAL_INT regs: OUT0=0x{int_out0:02X} OUT1=0x{int_out1:02X} CFG0=0x{int_cfg0:02X} CFG1=0x{int_cfg1:02X}"
        );

        Ok(())
    }

    fn clean(&mut self, c: u8, rep: u8) -> Result<()> {
        let data = match c {
            0 => 0b1010_1010, // white
            1 => 0b0101_0101, // black
            2 => 0b0000_0000, // discharge
            3 => 0b1111_1111, // skip
            _ => 0,
        };

        let send = self.encode_data(data);

        for _ in 0..rep {
            self.vscan_start()?;
            for _ in 0..E_INK_HEIGHT {
                self.hscan_start(send);

                // Match Arduino clean() cycle exactly
                self.gpio_out_set(send | CL_MASK);
                self.gpio_out_clear(CL_MASK);

                for _ in 0..(E_INK_WIDTH / 8 - 1) {
                    self.pulse_cl_only();
                    self.pulse_cl_only();
                }

                self.gpio_out_set(send | CL_MASK);
                self.gpio_out_clear(DATA_MASK | CL_MASK);
                self.vscan_end();
            }
            delay_us(230);
        }

        Ok(())
    }

    fn vscan_start(&mut self) -> Result<()> {
        self.set_ckv(true);
        delay_us(7);
        self.digital_write_internal(IO_INT_ADDR, SPV, false)?;
        delay_us(10);
        self.set_ckv(false);
        delay_us(1);
        self.set_ckv(true);
        delay_us(8);
        self.digital_write_internal(IO_INT_ADDR, SPV, true)?;
        delay_us(10);
        self.set_ckv(false);
        delay_us(1);
        self.set_ckv(true);
        delay_us(18);
        self.set_ckv(false);
        delay_us(1);
        self.set_ckv(true);
        delay_us(18);
        self.set_ckv(false);
        delay_us(1);
        self.set_ckv(true);
        Ok(())
    }

    #[inline(always)]
    fn vscan_end(&self) {
        self.set_ckv(false);
        self.set_le(true);
        self.set_le(false);
        delay_us(1);
    }

    #[inline(always)]
    fn hscan_start(&self, d: u32) {
        self.set_sph(false);
        self.write_data_and_clock(d);
        self.set_sph(true);
        self.set_ckv(true);
    }

    #[inline(always)]
    fn write_data_and_clock(&self, data_word: u32) {
        self.gpio_out_set(data_word | CL_MASK);
        self.gpio_out_clear(DATA_MASK | CL_MASK);
    }

    #[inline(always)]
    fn pulse_cl_only(&self) {
        self.gpio_out_set(CL_MASK);
        self.gpio_out_clear(CL_MASK);
    }

    #[inline(always)]
    fn clear_data_and_cl_le(&self) {
        self.gpio_out_clear(DATA_MASK | LE_MASK | CL_MASK);
    }

    #[inline(always)]
    fn set_cl(&self, high: bool) {
        if high {
            self.gpio_out_set(CL_MASK);
        } else {
            self.gpio_out_clear(CL_MASK);
        }
    }

    #[inline(always)]
    fn set_le(&self, high: bool) {
        if high {
            self.gpio_out_set(LE_MASK);
        } else {
            self.gpio_out_clear(LE_MASK);
        }
    }

    #[inline(always)]
    fn set_ckv(&self, high: bool) {
        if high {
            self.gpio_out1_set(CKV_MASK1);
        } else {
            self.gpio_out1_clear(CKV_MASK1);
        }
    }

    #[inline(always)]
    fn set_sph(&self, high: bool) {
        if high {
            self.gpio_out1_set(SPH_MASK1);
        } else {
            self.gpio_out1_clear(SPH_MASK1);
        }
    }

    #[inline(always)]
    fn gpio_out_set(&self, mask: u32) {
        unsafe { ptr::write_volatile(ptr::addr_of_mut!(sys::GPIO.out_w1ts), mask) }
    }

    #[inline(always)]
    fn gpio_out_clear(&self, mask: u32) {
        unsafe { ptr::write_volatile(ptr::addr_of_mut!(sys::GPIO.out_w1tc), mask) }
    }

    #[inline(always)]
    fn gpio_out1_set(&self, mask: u32) {
        unsafe { ptr::write_volatile(ptr::addr_of_mut!(sys::GPIO.out1_w1ts.val), mask) }
    }

    #[inline(always)]
    fn gpio_out1_clear(&self, mask: u32) {
        unsafe { ptr::write_volatile(ptr::addr_of_mut!(sys::GPIO.out1_w1tc.val), mask) }
    }

    fn pins_as_outputs(&mut self) -> Result<()> {
        for pin in [0i32, 2, 32, 33, 4, 5, 18, 19, 23, 25, 26, 27] {
            self.gpio_set_output(pin)?;
        }
        Ok(())
    }

    fn pins_z_state(&mut self) -> Result<()> {
        for pin in [2i32, 32, 33] {
            self.gpio_set_input(pin)?;
        }

        self.pin_mode_internal(IO_INT_ADDR, OE, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, GMOD, PinMode::Input)?;
        self.pin_mode_internal(IO_INT_ADDR, SPV, PinMode::Input)?;

        for pin in [0i32, 4, 5, 18, 19, 23, 25, 26, 27] {
            self.gpio_set_input(pin)?;
        }

        Ok(())
    }

    fn read_power_good(&mut self) -> Result<u8> {
        let mut buffer = [0u8; 1];
        self.i2c_write_read(TPS65186_ADDR, &[0x0F], &mut buffer)?;
        Ok(buffer[0])
    }

    fn encode_data(&self, data: u8) -> u32 {
        (((data & 0b0000_0011) as u32) << 4)
            | ((((data & 0b0000_1100) >> 2) as u32) << 18)
            | ((((data & 0b0001_0000) >> 4) as u32) << 23)
            | ((((data & 0b1110_0000) >> 5) as u32) << 25)
    }

    fn set_buzzer_frequency(&mut self, freq_hz: i32) -> Result<()> {
        if !ENABLE_BUZZER_PITCH_CONTROL {
            return Ok(());
        }

        let constrained = freq_hz.clamp(BEEP_FREQ_MIN_HZ, BEEP_FREQ_MAX_HZ);
        // Match the reference Arduino implementation.
        let wiper_percent = 156.499_576f32 + (-0.130_347_34f32 * constrained as f32);
        if !(0.0..=100.0).contains(&wiper_percent) {
            // Reference code skips digipot writes when mapped percent is out of bounds.
            return Ok(());
        }
        let wiper_value = ((wiper_percent / 100.0) * 127.0).round() as u8;

        let payload = [wiper_value & 0x7F];
        let mut last_err = None;

        // Retry with bus reset to recover from occasional NACK/invalid-state
        // conditions seen specifically on the buzzer digipot transaction.
        for attempt in 0..4 {
            match self.i2c_write(BUZZER_DIGIPOT_ADDR, &payload) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = Some(err);
                    if attempt == 0 {
                        match self.i2c_probe(BUZZER_DIGIPOT_ADDR) {
                            Ok(true) => eprintln!(
                                "W: Buzzer digipot 0x{BUZZER_DIGIPOT_ADDR:02X} ACKs probe but rejected write; retrying"
                            ),
                            Ok(false) => eprintln!(
                                "W: Buzzer digipot 0x{BUZZER_DIGIPOT_ADDR:02X} did not ACK probe; attempting recovery"
                            ),
                            Err(probe_err) => eprintln!(
                                "W: Buzzer digipot probe failed: {probe_err:?}; attempting recovery"
                            ),
                        }
                        // Keep the frontlight rail enabled during recovery in case
                        // this board revision shares supply gating.
                        let _ = self.frontlight_on();
                    }
                    if attempt < 3 {
                        let _ = self.i2c_reset_bus();
                        delay_ms(2);
                    }
                }
            }
        }

        if let Some(err) = last_err {
            Err(err)
        } else {
            Err(anyhow!("buzzer pitch write failed"))
        }
    }

    fn io_begin(&mut self, addr: u8) -> Result<()> {
        let mut regs = [0u8; 23];
        self.i2c_write_read(addr, &[0x00], &mut regs)?;
        match addr {
            IO_INT_ADDR => self.io_regs_int = regs,
            IO_EXT_ADDR => self.io_regs_ext = regs,
            _ => bail!("unsupported IO expander address 0x{addr:02X}"),
        }
        Ok(())
    }

    fn pin_mode_internal(&mut self, addr: u8, pin: u8, mode: PinMode) -> Result<()> {
        if pin > 15 {
            bail!("invalid expander pin {pin}");
        }

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
                // Match Arduino behavior: set output low before switching to output mode.
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

    fn digital_write_internal(&mut self, addr: u8, pin: u8, state: bool) -> Result<()> {
        if pin > 15 {
            bail!("invalid expander pin {pin}");
        }

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

    fn modify_cached_reg(
        &mut self,
        addr: u8,
        idx: usize,
        set_mask: u8,
        clear_mask: u8,
    ) -> Result<()> {
        let regs = match addr {
            IO_INT_ADDR => &mut self.io_regs_int,
            IO_EXT_ADDR => &mut self.io_regs_ext,
            _ => return Err(anyhow!("unsupported IO expander address 0x{addr:02X}")),
        };

        regs[idx] |= set_mask;
        regs[idx] &= !clear_mask;
        Ok(())
    }

    fn write_cached_reg(&mut self, addr: u8, idx: usize) -> Result<()> {
        let value = match addr {
            IO_INT_ADDR => self.io_regs_int[idx],
            IO_EXT_ADDR => self.io_regs_ext[idx],
            _ => bail!("unsupported IO expander address 0x{addr:02X}"),
        };

        self.i2c_write(addr, &[PCAL_REG_ADDRS[idx], value])
    }

    fn i2c_write(&mut self, addr: u8, bytes: &[u8]) -> Result<()> {
        self.i2c
            .write(addr, bytes)
            .with_context(|| format!("i2c write 0x{addr:02X}"))?;
        Ok(())
    }

    fn i2c_write_read(&mut self, addr: u8, bytes: &[u8], buffer: &mut [u8]) -> Result<()> {
        self.i2c
            .write_read(addr, bytes, buffer)
            .with_context(|| format!("i2c write_read 0x{addr:02X}"))?;
        Ok(())
    }

    fn i2c_reset_bus(&mut self) -> Result<()> {
        self.i2c.reset().context("i2c reset bus")
    }

    fn i2c_probe(&mut self, addr: u8) -> Result<bool> {
        self.i2c
            .probe(addr)
            .with_context(|| format!("i2c probe 0x{addr:02X}"))
    }

    fn read_i2c_reg(&mut self, addr: u8, reg: u8) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.i2c_write_read(addr, &[reg], &mut buf)?;
        Ok(buf[0])
    }

    fn gpio_set_output(&self, pin: i32) -> Result<()> {
        let reset_res = unsafe { sys::gpio_reset_pin(pin as sys::gpio_num_t) };
        if reset_res != 0 {
            bail!("gpio_reset_pin({pin}) failed: {reset_res}");
        }

        let cfg = sys::gpio_config_t {
            pin_bit_mask: 1u64 << (pin as u64),
            mode: sys::gpio_mode_t_GPIO_MODE_OUTPUT,
            pull_up_en: sys::gpio_pullup_t_GPIO_PULLUP_DISABLE,
            pull_down_en: sys::gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: sys::gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        let cfg_res = unsafe { sys::gpio_config(&cfg as *const sys::gpio_config_t) };
        if cfg_res != 0 {
            bail!("gpio_config({pin}) failed: {cfg_res}");
        }

        Ok(())
    }

    fn gpio_set_input(&self, pin: i32) -> Result<()> {
        let reset_res = unsafe { sys::gpio_reset_pin(pin as sys::gpio_num_t) };
        if reset_res != 0 {
            bail!("gpio_reset_pin({pin}) failed: {reset_res}");
        }

        let cfg = sys::gpio_config_t {
            pin_bit_mask: 1u64 << (pin as u64),
            mode: sys::gpio_mode_t_GPIO_MODE_INPUT,
            pull_up_en: sys::gpio_pullup_t_GPIO_PULLUP_DISABLE,
            pull_down_en: sys::gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
            intr_type: sys::gpio_int_type_t_GPIO_INTR_DISABLE,
        };
        let cfg_res = unsafe { sys::gpio_config(&cfg as *const sys::gpio_config_t) };
        if cfg_res != 0 {
            bail!("gpio_config({pin}) failed: {cfg_res}");
        }

        Ok(())
    }
}

impl OriginDimensions for Inkplate {
    fn size(&self) -> Size {
        Size::new(E_INK_WIDTH as u32, E_INK_HEIGHT as u32)
    }
}

impl DrawTarget for Inkplate {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> core::result::Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x < 0 || coord.y < 0 {
                continue;
            }

            let x = coord.x as usize;
            let y = coord.y as usize;
            if x < E_INK_WIDTH && y < E_INK_HEIGHT {
                // Panel/framebuffer orientation differs from embedded-graphics space.
                // Rotate CW to compensate observed 90deg CCW output on screen.
                let tx = E_INK_WIDTH - 1 - y;
                let ty = x;
                self.set_pixel_bw(tx, ty, matches!(color, BinaryColor::On));
            }
        }

        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> core::result::Result<(), Self::Error> {
        self.framebuffer_bw.fill(match color {
            BinaryColor::Off => 0x00,
            BinaryColor::On => 0xFF,
        });
        Ok(())
    }
}
