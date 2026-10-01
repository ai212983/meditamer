//! A bounded, single-attempt PUT held open for serial-correlated device probes.
use std::{
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

const PULSE_INTERVAL: Duration = Duration::from_secs(1);
const PACED_DEADLINE: Duration = Duration::from_secs(60);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_LIMIT: usize = 4096;

pub(crate) struct PacedUpload {
    stream: TcpStream,
    payload: [u8; 64],
    sent: usize,
    started: Instant,
    last_sent: Instant,
    finished: bool,
}

impl PacedUpload {
    pub(crate) fn start(
        address: SocketAddr,
        path: &str,
        token: Option<&str>,
        payload: [u8; 64],
    ) -> Result<Self> {
        validate_request(path, token)?;
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
            .context("paced upload connect failed")?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let token_header = token
            .map(|value| format!("x-upload-token: {value}\r\n"))
            .unwrap_or_default();
        let header = format!(
            "PUT /upload?path={path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: 64\r\n{token_header}\r\n"
        );
        let started = Instant::now();
        stream
            .write_all(header.as_bytes())
            .context("paced upload header write failed")?;
        stream
            .write_all(&payload[..1])
            .context("paced upload first byte failed")?;
        Ok(Self {
            stream,
            payload,
            sent: 1,
            started,
            last_sent: Instant::now(),
            finished: false,
        })
    }

    /// Called by the serial polling loop; only finish may complete the body.
    pub(crate) fn pulse(&mut self) -> Result<()> {
        self.check_pending()?;
        if self.last_sent.elapsed() >= PULSE_INTERVAL {
            self.stream
                .write_all(&self.payload[self.sent..self.sent + 1])
                .context("paced upload pulse failed")?;
            self.sent += 1;
            self.last_sent = Instant::now();
        }
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<Value> {
        self.check_pending()?;
        // Mark before I/O: a failed final write or response must not be retried.
        self.finished = true;
        let prefix_sent = self.sent;
        let paced_duration_ms = self.started.elapsed().as_millis() as u64;
        self.stream
            .write_all(&self.payload[self.sent..])
            .context("paced upload final bytes failed")?;
        self.sent = self.payload.len();
        let (status, body) = read_response(&mut self.stream)?;
        // This is the current direct PUT contract, not a stat acknowledgement.
        if status != 201 || body != b"upload ok" {
            bail!(
                "paced upload failed: HTTP {status} {}",
                String::from_utf8_lossy(&body)
            );
        }
        Ok(json!({
            "bytes": self.sent,
            "prefix_sent": prefix_sent,
            "paced_duration_ms": paced_duration_ms,
            "duration_ms": self.started.elapsed().as_millis() as u64,
            "http_status": status,
        }))
    }

    pub(crate) fn cancel(self) -> Result<Value> {
        self.check_pending()?;
        let report = json!({"prefix_sent": self.sent, "expected_bytes": self.payload.len(),
            "duration_ms": self.started.elapsed().as_millis() as u64});
        self.stream.shutdown(Shutdown::Both)?;
        Ok(report)
    }

    fn check_pending(&self) -> Result<()> {
        if self.finished {
            bail!("paced upload already finalized; no retries allowed");
        }
        if self.started.elapsed() >= PACED_DEADLINE {
            bail!("paced upload exceeded 60 second body deadline");
        }
        if self.sent >= self.payload.len() - 1 {
            bail!("paced upload exhausted its incomplete body prefix");
        }
        Ok(())
    }
}

impl Drop for PacedUpload {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

fn validate_request(path: &str, token: Option<&str>) -> Result<()> {
    if !path.starts_with("/assets/")
        || path.len() <= "/assets/".len()
        || path.len() > 128
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte))
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        bail!("paced upload requires a safe ASCII path under /assets/");
    }
    if token.is_some_and(|value| {
        value.len() > 256 || !value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
    }) {
        bail!("paced upload token contains invalid header bytes or exceeds 256 bytes");
    }
    Ok(())
}

fn read_response(stream: &mut TcpStream) -> Result<(u16, Vec<u8>)> {
    let started = Instant::now();
    let mut raw = [0; RESPONSE_LIMIT];
    let mut used = 0;
    let mut parsed = None;
    loop {
        if let Some((status, offset, length)) = parsed {
            if used >= offset + length {
                return Ok((status, raw[offset..offset + length].to_vec()));
            }
        }
        if used == raw.len() {
            bail!("paced upload response exceeds 4096 bytes");
        }
        let remaining = RESPONSE_TIMEOUT
            .checked_sub(started.elapsed())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| anyhow!("paced upload response exceeded 10 second deadline"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream
            .read(&mut raw[used..])
            .context("paced upload response read failed")?;
        if count == 0 {
            bail!("paced upload response ended before complete headers/body");
        }
        used += count;
        if parsed.is_none() {
            if let Some(end) = raw[..used].windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&raw[..end])
                    .context("paced upload response headers are not UTF-8")?;
                let (status, length) = response_headers(headers)?;
                let offset = end + 4;
                if length > RESPONSE_LIMIT - offset {
                    bail!("paced upload response exceeds 4096 bytes");
                }
                parsed = Some((status, offset, length));
            }
        }
    }
}

fn response_headers(headers: &str) -> Result<(u16, usize)> {
    let mut lines = headers.split("\r\n");
    let mut status_line = lines.next().unwrap_or_default().split_whitespace();
    if !matches!(status_line.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        bail!("paced upload response has invalid HTTP version");
    }
    let status = status_line
        .next()
        .ok_or_else(|| anyhow!("paced upload response has no status"))?
        .parse::<u16>()?;
    let mut length = None;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("paced upload response has malformed header"))?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            bail!("paced upload response uses unsupported transfer encoding");
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                bail!("paced upload response has duplicate Content-Length");
            }
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    Ok((
        status,
        length.ok_or_else(|| anyhow!("paced upload response has no Content-Length"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};

    fn server(response: Vec<u8>) -> (SocketAddr, thread::JoinHandle<(String, [u8; 64])>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
                assert!(header.len() < 4096);
            }
            let mut payload = [0; 64];
            stream.read_exact(&mut payload).unwrap();
            stream.write_all(&response).unwrap();
            (String::from_utf8(header).unwrap(), payload)
        });
        (address, worker)
    }

    fn response(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn sends_exact_payload_and_authenticated_header_then_accepts_created() {
        let (address, worker) = server(response("201 Created", "upload ok"));
        let payload = std::array::from_fn(|index| index as u8);
        let mut upload =
            PacedUpload::start(address, "/assets/obs1.bin", Some("test-token"), payload).unwrap();
        upload.pulse().unwrap();
        assert_eq!(upload.sent, 1);
        upload.last_sent -= PULSE_INTERVAL;
        upload.pulse().unwrap();
        assert_eq!(upload.sent, 2);
        let report = upload.finish().unwrap();
        assert_eq!(report["bytes"], 64);
        assert_eq!(report["prefix_sent"], 2);
        assert_eq!(report["http_status"], 201);
        assert!(upload.finish().is_err());
        let (header, actual) = worker.join().unwrap();
        assert_eq!(actual, payload);
        assert_eq!(header, format!("PUT /upload?path=/assets/obs1.bin HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: 64\r\nx-upload-token: test-token\r\n\r\n"));
    }

    #[test]
    fn rejects_http_failure_and_stat_acknowledgement() {
        for (status, body) in [
            ("503 Busy", "sd busy"),
            ("200 OK", "stat ok"),
            ("201 Created", "bad"),
        ] {
            let (address, worker) = server(response(status, body));
            let mut upload =
                PacedUpload::start(address, "/assets/obs2.bin", None, [42; 64]).unwrap();
            assert!(upload.finish().is_err());
            assert!(upload.finish().is_err());
            worker.join().unwrap();
        }
    }

    #[test]
    fn rejects_incomplete_and_oversized_responses() {
        for response in [
            b"HTTP/1.1 201 Created\r\nContent-Length: 9\r\n\r\nupload".to_vec(),
            b"HTTP/1.1 201 Created\r\nContent-Length: 4096\r\n\r\n".to_vec(),
            vec![b'x'; RESPONSE_LIMIT],
        ] {
            let (address, worker) = server(response);
            let mut upload =
                PacedUpload::start(address, "/assets/obs3.bin", None, [0; 64]).unwrap();
            assert!(upload.finish().is_err());
            worker.join().unwrap();
        }
    }

    #[test]
    fn rejects_invalid_header_inputs_before_connecting() {
        let address = "127.0.0.1:0".parse().unwrap();
        for path in [
            "/assets/a\r\nb",
            "/assets/a?b",
            "/assets/../a",
            "/other/a",
            "/assets/",
        ] {
            let error = PacedUpload::start(address, path, None, [0; 64])
                .err()
                .unwrap();
            assert!(error.to_string().contains("safe ASCII path"));
        }
        for token in ["abc\r\nx: y", "abc\ndef", "abc\0def"] {
            let error = PacedUpload::start(address, "/assets/a.bin", Some(token), [0; 64])
                .err()
                .unwrap();
            assert!(error.to_string().contains("invalid header bytes"));
        }
    }

    #[test]
    fn expired_or_exhausted_prefix_cannot_pulse_or_finish() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut upload = PacedUpload::start(
            listener.local_addr().unwrap(),
            "/assets/obs4.bin",
            None,
            [0; 64],
        )
        .unwrap();
        upload.started -= PACED_DEADLINE;
        assert!(upload.pulse().is_err());
        assert!(upload.finish().is_err());
        upload.started = Instant::now();
        upload.sent = 63;
        assert!(upload.pulse().is_err());
        assert!(upload.finish().is_err());
    }

    #[test]
    fn cancel_and_drop_close_an_incomplete_body() {
        for cancel in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let upload = PacedUpload::start(
                listener.local_addr().unwrap(),
                "/assets/obs5.bin",
                None,
                [42; 64],
            )
            .unwrap();
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            if cancel {
                let report = upload.cancel().unwrap();
                assert_eq!(report["prefix_sent"], 1);
                assert_eq!(report["expected_bytes"], 64);
            } else {
                drop(upload);
            }
            let mut request = Vec::new();
            stream.take(4096).read_to_end(&mut request).unwrap();
            let end = request
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .unwrap();
            assert_eq!(&request[end + 4..], &[42]);
        }
    }
}
