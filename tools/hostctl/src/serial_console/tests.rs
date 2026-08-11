use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};

use super::{sdreq_regex, SerialConsole};

#[derive(Clone, Default)]
struct MockState {
    reads: VecDeque<Vec<u8>>,
    writes: Vec<Vec<u8>>,
    dtr: Vec<bool>,
    rts: Vec<bool>,
    timeout: Duration,
}

#[derive(Clone, Default)]
struct MockPort {
    state: Arc<Mutex<MockState>>,
}

impl MockPort {
    fn new() -> Self {
        Self::default()
    }

    fn pushed_reads(self, chunks: &[&[u8]]) -> Self {
        let mut state = self.state.lock().expect("lock");
        state.reads = chunks.iter().map(|chunk| chunk.to_vec()).collect();
        drop(state);
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
        Err(io::Error::new(io::ErrorKind::TimedOut, "timeout"))
    }
}

impl Write for MockPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.state.lock().expect("lock").writes.push(buf.to_vec());
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

#[test]
fn sdreq_regex_matches_exact_op_token() {
    let fat_stat = sdreq_regex(Some("fat_stat")).expect("regex compiles");
    assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat"));
    assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat path=/foo"));
    assert!(!fat_stat.is_match("SDREQ id=7 op=fat_stat_extra"));
}

#[test]
fn capture_raw_for_reads_lines_from_same_descriptor() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[b"BOOT_RESET reason=poweron\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let bytes = console.capture_raw_for(Duration::from_millis(150))?;
    assert!(String::from_utf8_lossy(&bytes).contains("BOOT_RESET reason=poweron"));
    let lines = console.read_recent_lines(0);
    assert!(lines
        .iter()
        .any(|line| line.contains("BOOT_RESET reason=poweron")));
    Ok(())
}

#[test]
fn ack_detection_tolerates_a_concurrent_writer_prefix() -> Result<()> {
    let port =
        MockPort::new().pushed_reads(&[b"tap_trace,123,0x00UIFIXTURE OKtap_trace,124,0x00\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let (status, line) = console.wait_ack_since(0, "UIFIXTURE", Duration::from_millis(150))?;
    assert_eq!(status, super::AckStatus::Ok);
    assert_eq!(
        line.as_deref(),
        Some("tap_trace,123,0x00UIFIXTURE OKtap_trace,124,0x00")
    );
    Ok(())
}

#[test]
fn pulse_en_reset_toggles_only_rts_with_dtr_held_low() -> Result<()> {
    let port = MockPort::new();
    let state = port.state.clone();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    console.pulse_en_reset(1, 0)?;
    let state = state.lock().expect("lock");
    assert_eq!(state.dtr, vec![false]);
    assert_eq!(state.rts, vec![false, true, false]);
    Ok(())
}
