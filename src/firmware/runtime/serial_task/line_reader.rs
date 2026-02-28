use crate::firmware::config::TIMESET_CMD_BUF_LEN;

pub(super) enum LineReadEvent<'a> {
    None,
    Complete(&'a [u8]),
    Overflow,
}

pub(super) struct SerialLineReader {
    line_buf: [u8; TIMESET_CMD_BUF_LEN],
    line_len: usize,
    overflowed: bool,
}

impl SerialLineReader {
    pub(super) const fn new() -> Self {
        Self {
            line_buf: [0; TIMESET_CMD_BUF_LEN],
            line_len: 0,
            overflowed: false,
        }
    }

    pub(super) fn push_byte(&mut self, byte: u8) -> LineReadEvent<'_> {
        if byte == b'\r' || byte == b'\n' {
            if self.overflowed {
                self.overflowed = false;
                return LineReadEvent::None;
            }
            if self.line_len == 0 {
                return LineReadEvent::None;
            }
            let complete_len = self.line_len;
            self.line_len = 0;
            return LineReadEvent::Complete(&self.line_buf[..complete_len]);
        }

        if self.overflowed {
            return LineReadEvent::None;
        }

        if self.line_len < self.line_buf.len() {
            self.line_buf[self.line_len] = byte;
            self.line_len += 1;
            return LineReadEvent::None;
        }

        self.line_len = 0;
        self.overflowed = true;
        LineReadEvent::Overflow
    }
}

#[cfg(test)]
mod tests {
    use super::{LineReadEvent, SerialLineReader};

    #[test]
    fn emits_complete_line_on_newline() {
        let mut reader = SerialLineReader::new();
        assert!(matches!(reader.push_byte(b'A'), LineReadEvent::None));
        assert!(matches!(reader.push_byte(b'B'), LineReadEvent::None));
        match reader.push_byte(b'\n') {
            LineReadEvent::Complete(bytes) => assert_eq!(bytes, b"AB"),
            _ => panic!("expected complete line"),
        }
    }

    #[test]
    fn emits_single_overflow_and_drops_until_newline() {
        let mut reader = SerialLineReader::new();
        for _ in 0..crate::firmware::config::TIMESET_CMD_BUF_LEN {
            assert!(matches!(reader.push_byte(b'x'), LineReadEvent::None));
        }
        assert!(matches!(reader.push_byte(b'y'), LineReadEvent::Overflow));
        assert!(matches!(reader.push_byte(b'z'), LineReadEvent::None));
        assert!(matches!(reader.push_byte(b'\n'), LineReadEvent::None));
        assert!(matches!(reader.push_byte(b'a'), LineReadEvent::None));
        match reader.push_byte(b'\r') {
            LineReadEvent::Complete(bytes) => assert_eq!(bytes, b"a"),
            _ => panic!("expected complete line after overflow reset"),
        }
    }
}
