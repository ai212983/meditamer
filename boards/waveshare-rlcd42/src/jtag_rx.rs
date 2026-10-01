//! Non-owning USB-Serial-JTAG RX access for bounded runtime commands.
//!
//! The static PAC accessor reads the same FIFO `esp-println` uses without
//! claiming or resetting the peripheral. The runtime UI task is the sole
//! reader.

fn byte_ready() -> bool {
    esp_hal::peripherals::USB_DEVICE::regs()
        .ep1_conf()
        .read()
        .serial_out_ep_data_avail()
        .bit_is_set()
}

fn read_byte() -> u8 {
    esp_hal::peripherals::USB_DEVICE::regs().ep1().read().bits() as u8
}

/// Pops at most one currently available byte without waiting.
pub fn try_read_byte() -> Option<u8> {
    byte_ready().then(read_byte)
}
