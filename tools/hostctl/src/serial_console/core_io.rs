impl SerialConsole {
    pub fn poll_once(&mut self) -> Result<()> {
        let mut chunk = [0u8; 4096];
        loop {
            match self.port.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => self.handle_chunk(&chunk[..n])?,
                Err(err) if err.kind() == io::ErrorKind::TimedOut => break,
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(err).context("failed reading serial stream"),
            }
        }
        Ok(())
    }

    pub fn capture_raw_for(&mut self, duration: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + duration;
        let mut captured = Vec::new();
        let mut chunk = [0u8; 4096];
        while Instant::now() < deadline {
            match self.port.read(&mut chunk) {
                Ok(0) => {}
                Ok(n) => {
                    captured.extend_from_slice(&chunk[..n]);
                    self.handle_chunk(&chunk[..n])?;
                }
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err).context("failed reading serial stream"),
            }
        }
        Ok(captured)
    }

    fn handle_chunk(&mut self, chunk: &[u8]) -> Result<()> {
        self.rx_buf.extend_from_slice(chunk);
        self.normalize_and_extract_lines()
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
