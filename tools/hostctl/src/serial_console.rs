use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serialport::SerialPort;

const RX_BUF_MAX: usize = 16 * 1024;

pub struct SerialConsole {
    port: Box<dyn SerialPort>,
    rx_buf: Vec<u8>,
    lines: VecDeque<String>,
    line_cursor: usize,
    output: Option<File>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckStatus {
    Ok,
    Busy,
    Err,
    None,
}

impl SerialConsole {
    pub fn open(port: &str, baud: u32, output_path: Option<&Path>) -> Result<Self> {
        let mut serial = serialport::new(port, baud)
            .timeout(Duration::from_millis(50))
            .open()
            .with_context(|| format!("failed to open serial port {port} @ {baud}"))?;

        // Keep lines low to avoid forcing reset/boot mode transitions while attaching.
        let _ = serial.write_data_terminal_ready(false);
        let _ = serial.write_request_to_send(false);

        let output = match output_path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Some(File::create(path)?)
            }
            None => None,
        };

        Ok(Self {
            port: serial,
            rx_buf: Vec::with_capacity(1024),
            lines: VecDeque::new(),
            line_cursor: 0,
            output,
        })
    }

    #[cfg(test)]
    pub fn from_port_for_tests(
        port: Box<dyn SerialPort>,
        output_path: Option<&Path>,
    ) -> Result<Self> {
        let output = match output_path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Some(File::create(path)?)
            }
            None => None,
        };
        Ok(Self {
            port,
            rx_buf: Vec::with_capacity(1024),
            lines: VecDeque::new(),
            line_cursor: 0,
            output,
        })
    }

    pub fn send_line(&mut self, line: &str) -> Result<()> {
        self.port
            .write_all(line.as_bytes())
            .with_context(|| format!("failed to write serial payload: {line}"))?;
        self.port.write_all(b"\r\n")?;
        self.port.flush()?;
        Ok(())
    }

    pub fn settle(&mut self, settle_ms: u64) -> Result<()> {
        if settle_ms == 0 {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_millis(settle_ms);
        while Instant::now() < deadline {
            self.poll_once()?;
        }
        Ok(())
    }

    pub fn mark(&self) -> usize {
        self.line_cursor
    }

    pub fn read_recent_lines(&self, start_mark: usize) -> Vec<String> {
        let start = start_mark.min(self.line_cursor);
        let offset = self.line_cursor.saturating_sub(self.lines.len());
        let begin = start.saturating_sub(offset);
        self.lines
            .iter()
            .skip(begin)
            .cloned()
            .collect::<Vec<String>>()
    }

    pub fn wait_for_regex_since(
        &mut self,
        start_mark: usize,
        regex: &Regex,
        timeout: Duration,
    ) -> Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.poll_once()?;

            let recent = self.read_recent_lines(start_mark);
            if let Some(line) = recent.into_iter().find(|line| regex.is_match(line)) {
                return Ok(Some(line));
            }
        }
        Ok(None)
    }

    pub fn wait_ack_since(
        &mut self,
        start_mark: usize,
        ack_tag: &str,
        timeout: Duration,
    ) -> Result<(AckStatus, Option<String>)> {
        let pattern = Regex::new(&format!(
            r"^{} (OK|BUSY|ERR)(?:\b.*)?$",
            regex::escape(ack_tag)
        ))?;
        let line = self.wait_for_regex_since(start_mark, &pattern, timeout)?;
        let Some(line) = line else {
            return Ok((AckStatus::None, None));
        };

        if line.contains(" OK") {
            return Ok((AckStatus::Ok, Some(line)));
        }
        if line.contains(" BUSY") {
            return Ok((AckStatus::Busy, Some(line)));
        }
        if line.contains(" ERR") {
            return Ok((AckStatus::Err, Some(line)));
        }
        Ok((AckStatus::None, Some(line)))
    }

    pub fn find_first_regex_since(&self, start_mark: usize, regex: &Regex) -> Option<String> {
        self.read_recent_lines(start_mark)
            .into_iter()
            .find(|line| regex.is_match(line))
    }

    pub fn count_regex_since(&self, start_mark: usize, regex: &Regex) -> usize {
        self.read_recent_lines(start_mark)
            .into_iter()
            .filter(|line| regex.is_match(line))
            .count()
    }

    pub fn has_regex_since(&self, start_mark: usize, regex: &Regex) -> bool {
        self.find_first_regex_since(start_mark, regex).is_some()
    }

    pub fn last_regex_since(&self, start_mark: usize, regex: &Regex) -> Option<String> {
        self.read_recent_lines(start_mark)
            .into_iter()
            .filter(|line| regex.is_match(line))
            .last()
    }

    pub fn command_wait_regex(
        &mut self,
        command: &str,
        regex: &Regex,
        timeout: Duration,
    ) -> Result<Option<String>> {
        let mark = self.mark();
        self.send_line(command)?;
        self.wait_for_regex_since(mark, regex, timeout)
    }

    pub fn command_wait_ack(
        &mut self,
        command: &str,
        ack_tag: &str,
        timeout: Duration,
    ) -> Result<(AckStatus, Option<String>)> {
        let mark = self.mark();
        self.send_line(command)?;
        self.wait_ack_since(mark, ack_tag, timeout)
    }

    pub fn wait_for_sdreq_id_since(
        &mut self,
        start_mark: usize,
        op: Option<&str>,
        timeout: Duration,
    ) -> Result<Option<u32>> {
        let pattern = sdreq_regex(op)?;
        let line = self.wait_for_regex_since(start_mark, &pattern, timeout)?;
        let Some(line) = line else {
            return Ok(None);
        };

        let capture = Regex::new(r"id=([0-9]+)")?
            .captures(&line)
            .ok_or_else(|| anyhow!("failed to parse SDREQ id from line: {line}"))?;
        let id = capture
            .get(1)
            .ok_or_else(|| anyhow!("missing SDREQ capture group"))?
            .as_str()
            .parse::<u32>()?;
        Ok(Some(id))
    }

    pub fn sdwait_for_id(&mut self, id: u32, timeout_ms: u32) -> Result<Option<String>> {
        let mark = self.mark();
        self.send_line(&format!("SDWAIT {id} {timeout_ms}"))?;
        let timeout = Duration::from_secs((timeout_ms as u64 / 1000) + 15);
        let pattern = Regex::new(r"^SDWAIT (DONE|TIMEOUT|ERR)")?;
        self.wait_for_regex_since(mark, &pattern, timeout)
    }

    pub fn poll_once(&mut self) -> Result<()> {
        let mut chunk = [0u8; 4096];
        loop {
            match self.port.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    self.rx_buf.extend_from_slice(&chunk[..n]);
                    self.normalize_and_extract_lines()?;
                }
                Err(err) if err.kind() == io::ErrorKind::TimedOut => break,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err).context("failed reading serial stream"),
            }
        }
        Ok(())
    }

    fn normalize_and_extract_lines(&mut self) -> Result<()> {
        for byte in &mut self.rx_buf {
            if *byte == b'\r' {
                *byte = b'\n';
            }
        }

        while let Some(pos) = self.rx_buf.iter().position(|b| *b == b'\n') {
            let mut line = self.rx_buf.drain(..=pos).collect::<Vec<u8>>();
            while matches!(line.last(), Some(b'\n')) {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }

            let parsed = String::from_utf8_lossy(&line).trim().to_string();
            if parsed.is_empty() {
                continue;
            }

            if let Some(file) = &mut self.output {
                writeln!(file, "{parsed}")?;
                file.flush()?;
            }

            self.lines.push_back(parsed);
            self.line_cursor += 1;
            while self.lines.len() > RX_BUF_MAX {
                self.lines.pop_front();
            }
        }

        Ok(())
    }
}

fn sdreq_regex(op: Option<&str>) -> Result<Regex> {
    match op {
        Some(op) => Regex::new(&format!(r"^SDREQ id=([0-9]+) op={}\b", regex::escape(op)))
            .map_err(Into::into),
        None => Regex::new(r"^SDREQ id=([0-9]+) op=").map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdreq_regex_matches_exact_op_token() {
        let fat_stat = sdreq_regex(Some("fat_stat")).expect("regex compiles");
        assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat"));
        assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat path=/foo"));
        assert!(!fat_stat.is_match("SDREQ id=7 op=fat_stat_extra"));
    }
}
