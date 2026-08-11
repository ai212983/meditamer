use core::fmt::Write;

use embassy_time::{Duration, Timer};
use esp_hal::uart::Config as UartConfig;

use super::task_state::SerialTaskState;
use crate::firmware::{
    config::UART_BAUD,
    firmware_update::{self, STREAM_CHUNK_MAX},
    touch::debug_log::uart_write_all,
    types::SerialUart,
};

pub(super) const STREAM_BAUD: u32 = 460_800;
const MAGIC: [u8; 2] = *b"MF";
const VERSION: u8 = 1;
const KIND_DATA: u8 = 1;
const HEADER_LEN: usize = 10;
const CRC_LEN: usize = 4;
const FRAME_MAX: usize = HEADER_LEN + STREAM_CHUNK_MAX + CRC_LEN;
const BAUD_SWITCH_DELAY_MS: u64 = 20;

pub(super) enum FrameReadEvent<'a> {
    None,
    Complete { offset: u32, payload: &'a [u8] },
    Error(&'static str),
}

pub(super) struct FirmwareFrameReader {
    buffer: [u8; FRAME_MAX],
    len: usize,
    expected: usize,
}

impl FirmwareFrameReader {
    pub(super) const fn new() -> Self {
        Self {
            buffer: [0; FRAME_MAX],
            len: 0,
            expected: 0,
        }
    }

    pub(super) fn reset(&mut self) {
        self.len = 0;
        self.expected = 0;
    }

    pub(super) fn push_byte(&mut self, byte: u8) -> FrameReadEvent<'_> {
        if self.len == 0 {
            if byte == MAGIC[0] {
                self.buffer[0] = byte;
                self.len = 1;
            }
            return FrameReadEvent::None;
        }
        if self.len == 1 {
            if byte != MAGIC[1] {
                self.len = usize::from(byte == MAGIC[0]);
                return FrameReadEvent::None;
            }
            self.buffer[1] = byte;
            self.len = 2;
            return FrameReadEvent::None;
        }

        self.buffer[self.len] = byte;
        self.len += 1;
        if self.len == HEADER_LEN {
            let payload_len = u16::from_le_bytes([self.buffer[8], self.buffer[9]]) as usize;
            if self.buffer[2] != VERSION
                || self.buffer[3] != KIND_DATA
                || payload_len == 0
                || payload_len > STREAM_CHUNK_MAX
                || !payload_len.is_multiple_of(4)
            {
                self.reset();
                return FrameReadEvent::Error("header");
            }
            self.expected = HEADER_LEN + payload_len + CRC_LEN;
        }
        if self.expected == 0 || self.len < self.expected {
            return FrameReadEvent::None;
        }

        let payload_end = self.expected - CRC_LEN;
        let received_crc =
            u32::from_le_bytes(self.buffer[payload_end..self.expected].try_into().unwrap());
        if crc32(&self.buffer[2..payload_end]) != received_crc {
            self.reset();
            return FrameReadEvent::Error("crc");
        }
        let offset = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap());
        self.len = 0;
        self.expected = 0;
        FrameReadEvent::Complete {
            offset,
            payload: &self.buffer[HEADER_LEN..payload_end],
        }
    }
}

pub(super) async fn begin_stream(uart: &mut SerialUart, state: &mut SerialTaskState, baud: u32) {
    if baud != STREAM_BAUD {
        let _ = uart_write_all(uart, b"FWSTREAM ERR reason=baud\r\n").await;
        return;
    }
    match firmware_update::prepare_stream() {
        Ok(erased) => {
            let mut line = heapless::String::<96>::new();
            let _ = write!(
                &mut line,
                "FWSTREAM OK baud={} version={} payload={} erased={}\r\n",
                STREAM_BAUD, VERSION, STREAM_CHUNK_MAX, erased,
            );
            let _ = uart_write_all(uart, line.as_bytes()).await;
            let _ = uart.flush_async().await;
            Timer::after(Duration::from_millis(BAUD_SWITCH_DELAY_MS)).await;
            if uart
                .apply_config(&UartConfig::default().with_baudrate(STREAM_BAUD))
                .is_ok()
            {
                state.begin_firmware_stream();
            } else {
                let _ = uart.apply_config(&UartConfig::default().with_baudrate(UART_BAUD));
            }
        }
        Err(error) => {
            let mut line = heapless::String::<64>::new();
            let _ = write!(&mut line, "FWSTREAM ERR reason={}\r\n", error.label());
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
    }
}

pub(super) async fn handle_frame(
    uart: &mut SerialUart,
    state: &mut SerialTaskState,
    event: FrameReadEvent<'_>,
) {
    match event {
        FrameReadEvent::None => {}
        FrameReadEvent::Error(reason) => {
            let mut line = heapless::String::<48>::new();
            let _ = write!(&mut line, "FWFRAME ERR reason={reason}\r\n");
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
        FrameReadEvent::Complete { offset, payload } => {
            match firmware_update::write_chunk(offset, payload) {
                Ok(written) => {
                    let complete = firmware_update::stream_complete();
                    let mut line = heapless::String::<64>::new();
                    let _ = write!(&mut line, "FWFRAME OK written={written}\r\n");
                    let _ = uart_write_all(uart, line.as_bytes()).await;
                    if complete {
                        let _ = uart.flush_async().await;
                        Timer::after(Duration::from_millis(BAUD_SWITCH_DELAY_MS)).await;
                        let _ = uart.apply_config(&UartConfig::default().with_baudrate(UART_BAUD));
                        state.end_firmware_stream();
                    }
                }
                Err(error) => {
                    let mut line = heapless::String::<64>::new();
                    let _ = write!(&mut line, "FWFRAME ERR reason={}\r\n", error.label());
                    let _ = uart_write_all(uart, line.as_bytes()).await;
                }
            }
        }
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    fn frame(offset: u32, payload: &[u8]) -> heapless::Vec<u8, FRAME_MAX> {
        let mut bytes = heapless::Vec::new();
        bytes.extend_from_slice(&MAGIC).unwrap();
        bytes.push(VERSION).unwrap();
        bytes.push(KIND_DATA).unwrap();
        bytes.extend_from_slice(&offset.to_le_bytes()).unwrap();
        bytes
            .extend_from_slice(&(payload.len() as u16).to_le_bytes())
            .unwrap();
        bytes.extend_from_slice(payload).unwrap();
        let crc = crc32(&bytes[2..]);
        bytes.extend_from_slice(&crc.to_le_bytes()).unwrap();
        bytes
    }

    #[test]
    fn parses_a_binary_data_frame() {
        let payload = [0x5a; STREAM_CHUNK_MAX];
        let frame = frame(480, &payload);
        let mut reader = FirmwareFrameReader::new();
        for byte in &frame[..frame.len() - 1] {
            assert!(matches!(reader.push_byte(*byte), FrameReadEvent::None));
        }
        match reader.push_byte(frame[frame.len() - 1]) {
            FrameReadEvent::Complete {
                offset,
                payload: decoded,
            } => {
                assert_eq!(offset, 480);
                assert_eq!(decoded, payload);
            }
            _ => panic!("expected complete frame"),
        }
    }

    #[test]
    fn rejects_a_corrupt_frame_and_resynchronizes() {
        let payload = [0xa5; 4];
        let mut bad = frame(0, &payload);
        let last = bad.len() - 1;
        bad[last] ^= 1;
        let good = frame(4, &payload);
        let mut reader = FirmwareFrameReader::new();
        let mut saw_crc = false;
        for byte in bad.iter().chain(good.iter()) {
            match reader.push_byte(*byte) {
                FrameReadEvent::Error("crc") => saw_crc = true,
                FrameReadEvent::Complete { offset, payload } => {
                    assert!(saw_crc);
                    assert_eq!(offset, 4);
                    assert_eq!(payload, [0xa5; 4]);
                    return;
                }
                _ => {}
            }
        }
        panic!("good frame was not recovered");
    }
}
