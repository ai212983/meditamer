use crate::firmware::config::SD_SERIAL_LINES;

pub(super) type SdSerialLine = heapless::String<256>;

pub(super) fn send(line: SdSerialLine) -> bool {
    SD_SERIAL_LINES.try_send(line).is_ok()
}
