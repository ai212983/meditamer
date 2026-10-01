//! Shared fake serial port for host tests.
//! Moved verbatim from `tests.rs` so workflow tests can drive
//! `SerialConsole` without duplicating the stub.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};

#[derive(Clone, Default)]
pub(crate) struct MockState {
    pub(crate) reads: VecDeque<Vec<u8>>,
    pub(crate) writes: Vec<Vec<u8>>,
    pub(crate) dtr: Vec<bool>,
    pub(crate) rts: Vec<bool>,
    pub(crate) timeout: Duration,
    pub(crate) fail_writes: bool,
    pub(crate) repeat_read: bool,
}

#[derive(Clone, Default)]
pub(crate) struct MockPort {
    pub(crate) state: Arc<Mutex<MockState>>,
}

impl MockPort {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn pushed_reads(self, chunks: &[&[u8]]) -> Self {
        let mut state = self.state.lock().expect("lock");
        state.reads = chunks.iter().map(|chunk| chunk.to_vec()).collect();
        drop(state);
        self
    }

    pub(crate) fn failing_writes(self) -> Self {
        self.state.lock().expect("lock").fail_writes = true;
        self
    }
}
impl Read for MockPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut state = self.state.lock().expect("lock");
        if let Some(chunk) = state.reads.pop_front() {
            let len = chunk.len().min(buf.len());
            buf[..len].copy_from_slice(&chunk[..len]);
            return Ok(len);
        }
        if state.repeat_read {
            buf[..5].copy_from_slice(b"LOG\r\n");
            return Ok(5);
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"))
    }
}
impl Write for MockPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.state.lock().expect("lock");
        if state.fail_writes {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failed"));
        }
        state.writes.push(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl SerialPort for MockPort {
    fn name(&self) -> Option<String> {
        Some("mock".into())
    }
    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(115200)
    }
    fn data_bits(&self) -> serialport::Result<DataBits> {
        Ok(DataBits::Eight)
    }
    fn flow_control(&self) -> serialport::Result<FlowControl> {
        Ok(FlowControl::None)
    }
    fn parity(&self) -> serialport::Result<Parity> {
        Ok(Parity::None)
    }
    fn stop_bits(&self) -> serialport::Result<StopBits> {
        Ok(StopBits::One)
    }
    fn timeout(&self) -> Duration {
        self.state.lock().expect("lock").timeout
    }
    fn set_baud_rate(&mut self, _baud_rate: u32) -> serialport::Result<()> {
        Ok(())
    }
    fn set_data_bits(&mut self, _data_bits: DataBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_flow_control(&mut self, _flow_control: FlowControl) -> serialport::Result<()> {
        Ok(())
    }
    fn set_parity(&mut self, _parity: Parity) -> serialport::Result<()> {
        Ok(())
    }
    fn set_stop_bits(&mut self, _stop_bits: StopBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.state.lock().expect("lock").timeout = timeout;
        Ok(())
    }
    fn write_request_to_send(&mut self, level: bool) -> serialport::Result<()> {
        self.state.lock().expect("lock").rts.push(level);
        Ok(())
    }
    fn write_data_terminal_ready(&mut self, level: bool) -> serialport::Result<()> {
        self.state.lock().expect("lock").dtr.push(level);
        Ok(())
    }
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        Ok(0)
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
    }
    fn clear(&self, _buffer_to_clear: ClearBuffer) -> serialport::Result<()> {
        Ok(())
    }
    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        Ok(Box::new(self.clone()))
    }
    fn set_break(&self) -> serialport::Result<()> {
        Ok(())
    }
    fn clear_break(&self) -> serialport::Result<()> {
        Ok(())
    }
}
