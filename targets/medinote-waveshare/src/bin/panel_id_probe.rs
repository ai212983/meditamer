//! Reads the RLCD controller's ID registers, to settle whether the die on the
//! Waveshare ESP32-S3-RLCD-4.2 is the ST7305 the vendor driver names or an
//! ST7306-class sibling
//! (`docs/references/hardware/waveshare-rlcd42/panel.md`, "ST7305 or ST7306?").
//!
//! The question is open because the datasheet Waveshare publishes for this
//! board does not match this board: `ST_7305_V0_2.pdf` documents a 264x320
//! part, and the vendor's own init drives past its CASET range, its RASET
//! range, and its GATESET table. Whatever is on the glass has more RAM than
//! the documented part.
//!
//! **Why this cannot use `Spi`.** The product configures SPI2 with SCK and
//! MOSI only — a write-only 4-wire bus. Reading needs SDA turned around:
//! §7.1.4 defines it as "serial data input/output", and the panel drives the
//! same GPIO12 line during a read. So this bit-bangs GPIO11/GPIO12 directly
//! rather than fighting the SPI peripheral's duplex configuration. It is a
//! standalone binary for that reason and never links against the panel driver.
//!
//! **What a result proves, and what it does not.** `RDDID`/`RDID1..3` are
//! loaded from the NV memory the datasheet describes as holding the "Factory
//! Default Value (Module ID, Module Version)". They therefore identify the
//! *module*, not necessarily the die. A recognisable code is strong evidence
//! and a fingerprint worth recording; it is not proof on its own. The
//! decisive test remains whether this glass can show four grey levels.
//!
//! Three deliberate design choices, each because a naive probe would produce a
//! confident wrong answer:
//!
//! * **Every read runs twice, once with the bus pulled up and once pulled
//!   down.** If the two disagree the line is floating — the panel is not
//!   driving it, so the read is unsupported or the die is not wired for it,
//!   and any "ID" is just the pull resistor. If they agree the panel really is
//!   driving. Without this, an all-zeros read is indistinguishable from a
//!   genuine `0x00`.
//! * **32 raw bits are captured and decoded at two byte alignments.** The
//!   datasheet's read figures are images, so whether a status read begins with
//!   a dummy bit (RAM reads do) could not be established from the text. Rather
//!   than guess and reflash, both alignments are printed and the operator
//!   picks the one that reads sensibly.
//! * **The read is repeated at three points in bring-up.** `NVMLOADCTRL`
//!   (`0xD6`) gates whether the NV load is triggered by timer or by sleep-out,
//!   so the IDs may be absent before the vendor's `0xD6` and before `0x11`.
//!   Reading at all three shows when they land instead of assuming.
//!
//! Build and flash:
//!
//! ```text
//! targets/medinote-waveshare/build.sh --locked --bin panel-id-probe
//! ESPFLASH_PORT=/dev/cu.usbmodem21101 espflash flash --chip esp32s3 \
//!   --partition-table targets/medinote-waveshare/partitions.csv \
//!   --target-app-partition factory \
//!   targets/medinote-waveshare/target/xtensa-esp32s3-none-elf/release/panel-id-probe
//! ```

#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Flex, InputConfig, Level, Output, OutputConfig, Pull};

/// See `main.rs`'s identical constant: esp-println's jtag-serial backend drops
/// output written before the USB CDC host finishes enumerating.
const CONSOLE_SETTLE_MS: u32 = 800;

/// Half a bit period. The datasheet's minimum read pulse width is 60 ns in
/// each direction, so 1 us is roughly 16x slack — this probe has no reason to
/// be fast and every reason to be unambiguous.
const HALF_CYCLE_US: u32 = 1;

/// Bits captured per read. Three ID parameters need 24, plus slack for a
/// dummy bit and for seeing what follows.
const READ_BITS: u32 = 32;

/// Commands under test. `RDDID` returns all three ID bytes in one transaction;
/// the individual `RDID1..3` are read as well because a die that answers one
/// and not the other is itself a finding.
const READS: [(u8, &str); 4] = [
    (0x04, "RDDID"),
    (0xDA, "RDID1"),
    (0xDB, "RDID2"),
    (0xDC, "RDID3"),
];

struct PanelBus<'d> {
    sclk: Output<'d>,
    sda: Flex<'d>,
    dc: Output<'d>,
    cs: Output<'d>,
    reset: Output<'d>,
    delay: Delay,
}

impl<'d> PanelBus<'d> {
    fn tick(&mut self) {
        self.delay.delay_micros(HALF_CYCLE_US);
    }

    fn sda_drive(&mut self) {
        self.sda.set_input_enable(false);
        self.sda.set_output_enable(true);
    }

    fn sda_listen(&mut self, pull: Pull) {
        self.sda.set_output_enable(false);
        self.sda
            .apply_input_config(&InputConfig::default().with_pull(pull));
        self.sda.set_input_enable(true);
    }

    /// MSB first, data presented while the clock is low and latched by the
    /// panel on the rising edge (§7.1.4).
    fn write_byte(&mut self, byte: u8) {
        for index in (0..8).rev() {
            self.sclk.set_low();
            if (byte >> index) & 1 == 1 {
                self.sda.set_high();
            } else {
                self.sda.set_low();
            }
            self.tick();
            self.sclk.set_high();
            self.tick();
        }
        self.sclk.set_low();
    }

    fn command(&mut self, command: u8) {
        self.cs.set_low();
        self.dc.set_low();
        self.sda_drive();
        self.write_byte(command);
        self.cs.set_high();
    }

    fn command_with(&mut self, command: u8, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_low();
        self.sda_drive();
        self.write_byte(command);
        self.dc.set_high();
        for byte in data {
            self.write_byte(*byte);
        }
        self.cs.set_high();
    }

    /// Send `command`, turn the bus around, and clock out [`READ_BITS`] bits.
    ///
    /// CS is held low across the turnaround, which the datasheet requires:
    /// "CSB signal must be kept at 'L' during this period." The panel is
    /// assumed to present each bit on the falling edge, so sampling happens
    /// after the rising edge, when the level is stable. Being off by one edge
    /// only shifts the result, which the two-alignment decode already covers.
    fn read_raw(&mut self, command: u8, pull: Pull) -> u32 {
        self.cs.set_low();
        self.dc.set_low();
        self.sda_drive();
        self.write_byte(command);

        self.dc.set_high();
        self.sda_listen(pull);
        self.tick();

        let mut captured = 0u32;
        for _ in 0..READ_BITS {
            self.sclk.set_low();
            self.tick();
            self.sclk.set_high();
            self.tick();
            captured = (captured << 1) | u32::from(self.sda.is_high());
        }

        self.sclk.set_low();
        self.cs.set_high();
        // Leave the bus driven so an idle probe does not float the panel's
        // input between stages.
        self.sda_drive();
        captured
    }

    fn hardware_reset(&mut self) {
        self.reset.set_high();
        self.delay.delay_millis(50);
        self.reset.set_low();
        self.delay.delay_millis(20);
        self.reset.set_high();
        self.delay.delay_millis(50);
    }
}

/// Three ID bytes read out of `captured` assuming `dummy_bits` leading bits
/// belong to the bus turnaround rather than to the data.
fn decode(captured: u32, dummy_bits: u32) -> [u8; 3] {
    let aligned = captured << dummy_bits;
    [
        (aligned >> 24) as u8,
        (aligned >> 16) as u8,
        (aligned >> 8) as u8,
    ]
}

fn probe(bus: &mut PanelBus<'_>, stage: &str) {
    for (command, name) in READS {
        let up = bus.read_raw(command, Pull::Up);
        let down = bus.read_raw(command, Pull::Down);
        // Agreement across opposite pulls is the whole test for "is anything
        // actually driving this line".
        let driven = up == down;
        let d0 = decode(up, 0);
        let d1 = decode(up, 1);

        console::println!(
            "PANEL_ID_PROBE stage={} cmd=0x{:02X} name={} driven={} up=0x{:08X} down=0x{:08X} \
             dummy0=[{:02X} {:02X} {:02X}] dummy1=[{:02X} {:02X} {:02X}]",
            stage,
            command,
            name,
            if driven { "yes" } else { "no-floating" },
            up,
            down,
            d0[0],
            d0[1],
            d0[2],
            d1[0],
            d1[1],
            d1[2]
        );
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    let delay = Delay::new();
    delay.delay_millis(CONSOLE_SETTLE_MS);
    console::println!("PANEL_ID_PROBE board=waveshare-rlcd42 chip=esp32s3 state=boot");
    console::println!(
        "PANEL_ID_PROBE pins sclk=GPIO11 sda=GPIO12 dc=GPIO5 cs=GPIO40 reset=GPIO41 \
         half_cycle_us={} read_bits={}",
        HALF_CYCLE_US,
        READ_BITS
    );

    let output = OutputConfig::default();
    let mut sda = Flex::new(peripherals.GPIO12);
    sda.apply_output_config(&output);
    sda.set_low();
    sda.set_output_enable(true);

    let mut bus = PanelBus {
        sclk: Output::new(peripherals.GPIO11, Level::Low, output),
        sda,
        dc: Output::new(peripherals.GPIO5, Level::Low, output),
        cs: Output::new(peripherals.GPIO40, Level::High, output),
        reset: Output::new(peripherals.GPIO41, Level::High, output),
        delay,
    };

    bus.hardware_reset();
    console::println!("PANEL_ID_PROBE state=reset_done");
    probe(&mut bus, "after_reset");

    // The vendor's own first init command. NVMLOADCTRL selects whether the NV
    // load — which is what carries the module ID — is triggered by timer or by
    // sleep-out, so the IDs may only appear after this.
    bus.command_with(0xD6, &[0x17, 0x02]);
    console::println!("PANEL_ID_PROBE state=nvmloadctrl_sent");
    probe(&mut bus, "after_nvmloadctrl");

    bus.command(0x11); // sleep out
    bus.delay.delay_millis(120);
    console::println!("PANEL_ID_PROBE state=sleep_out_done");
    probe(&mut bus, "after_sleep_out");

    console::println!("PANEL_ID_PROBE state=complete");
    loop {
        bus.delay.delay_millis(1000);
    }
}
