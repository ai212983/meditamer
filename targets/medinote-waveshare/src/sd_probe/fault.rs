//! Input-only fault injection for this sole-owner SD probe. Never drive DAT0.
//! Completion delivery is held while changing the receiver input; no DMA
//! descriptors or buffers are modified. The caller drops the HAL future and
//! quiesces the engine before restoring the saved mapping on cancellation.

use super::policy::Fault;
use esp_hal::{
    gpio::{InputSignal, Level},
    time::{Duration, Instant},
};

const DATA_OVER: u32 = 1 << 3;
const DATA_CRC: u32 = 1 << 7;
const ERRORS: u32 = (1 << 1)
    | (1 << 6)
    | (1 << 7)
    | (1 << 8)
    | (1 << 9)
    | (1 << 10)
    | (1 << 11)
    | (1 << 12)
    | (1 << 13)
    | (1 << 15);
const DAT0: InputSignal = InputSignal::SDHOST_CDATA_IN_20;

pub(super) struct Injection {
    mode: Fault,
    saved_input: u32,
    triggered: bool,
    busy_armed: bool,
    raw_status: u32,
    inverted_at: u32,
    restored_at: u32,
}

impl Injection {
    pub fn new(mode: Fault) -> Self {
        let saved_input = esp_hal::peripherals::GPIO::regs()
            .func_in_sel_cfg(DAT0 as usize)
            .read()
            .bits();
        if mode != Fault::None {
            super::recovery::set_interrupt_delivery(false);
        }
        Self {
            mode,
            saved_input,
            triggered: false,
            busy_armed: false,
            raw_status: 0,
            inverted_at: 0,
            restored_at: 0,
        }
    }

    fn restore_input(&self) {
        // SAFETY: restore the exact receiver mapping owned by this probe. This
        // changes no output routing, pin mode, DMA address, or borrowed buffer.
        esp_hal::peripherals::GPIO::regs()
            .func_in_sel_cfg(DAT0 as usize)
            .write(|w| unsafe { w.bits(self.saved_input) });
    }

    /// Called after polling the transfer; registers are only observed while
    /// pending. Global SDHOST interrupt delivery stays masked during mutation.
    pub fn poll_pending(&mut self) {
        let r = esp_hal::peripherals::SDHOST::regs();
        match self.mode {
            Fault::WriteBusy => {
                if !self.triggered {
                    let status = r.rintsts().read().bits();
                    if status & DATA_OVER != 0 && status & ERRORS == 0 {
                        self.raw_status = status;
                        DAT0.connect_to(&Level::Low);
                        self.triggered = true;
                        super::recovery::set_interrupt_delivery(true);
                    }
                } else if r.cardthrctl().read().cardclrinten().bit_is_set() {
                    self.busy_armed = true;
                }
            }
            Fault::ReadCrc if !self.triggered => {
                self.triggered = true;
                // One 512-byte read at 1 MHz takes about 4 ms. Observe CIU byte
                // progress directly so executor scheduling cannot skip the
                // payload window. This diagnostic poll has its own 20 ms cap.
                let deadline = Instant::now() + Duration::from_millis(20);
                while Instant::now() < deadline {
                    let count = r.tcbcnt().read().bits();
                    let status = r.rintsts().read().bits();
                    if status & (DATA_OVER | ERRORS) != 0 {
                        break;
                    }
                    if count >= 128 {
                        if count < 256 {
                            self.inverted_at = count;
                            // SAFETY: input-only inversion while completion is
                            // masked; the card's output is never driven here.
                            esp_hal::peripherals::GPIO::regs()
                                .func_in_sel_cfg(DAT0 as usize)
                                .write(|w| unsafe { w.bits(self.saved_input ^ (1 << 6)) });
                            while r.tcbcnt().read().bits() < 256
                                && Instant::now() < deadline
                                && r.rintsts().read().bits() & ERRORS == 0
                            {
                                core::hint::spin_loop();
                            }
                            self.restored_at = r.tcbcnt().read().bits();
                            self.restore_input();
                        }
                        break;
                    }
                    core::hint::spin_loop();
                }
                self.restore_input();
                while r.rintsts().read().bits() & (DATA_OVER | ERRORS) == 0
                    && Instant::now() < deadline
                {
                    core::hint::spin_loop();
                }
                self.raw_status = r.rintsts().read().bits();
                super::recovery::set_interrupt_delivery(true);
            }
            _ => {}
        }
    }

    /// Called only after the operation has dropped and DMA is quiescent.
    pub fn finish(&self) -> bool {
        if self.mode == Fault::None {
            return true;
        }
        self.restore_input();
        super::recovery::set_interrupt_delivery(true);
        console::println!("SD_PROBE state=fault_observed mode={:?} triggered={} busy_armed={} raw_status={:#x} inverted_at={} restored_at={} input_restored=true", self.mode, self.triggered, self.busy_armed, self.raw_status, self.inverted_at, self.restored_at);
        match self.mode {
            Fault::None | Fault::MissingCompletion => true,
            Fault::WriteBusy => self.triggered && self.busy_armed,
            Fault::ReadCrc => {
                self.inverted_at >= 128
                    && self.inverted_at < 256
                    && self.restored_at >= 256
                    && self.restored_at < 512
                    && self.raw_status & ERRORS == DATA_CRC
            }
        }
    }
}
