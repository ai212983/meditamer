use std::{
    io::{ErrorKind, Read, Write},
    net::{Shutdown, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::Method;

const HTTP_HEADER_END: &[u8] = b"\r\n\r\n";
const HTTP_HEADER_MAX_BYTES: usize = 64 * 1024;
const HTTP_READ_BUFFER_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DirectStreamBodyCadence {
    pub(super) read_calls: u32,
    pub(super) short_reads: u32,
    pub(super) read_bytes: usize,
    pub(super) read_gap_ms_total: u32,
    pub(super) read_gap_ms_max: u32,
    pub(super) read_gap_over_10ms: u32,
    pub(super) read_gap_over_50ms: u32,
}

#[derive(Debug)]
pub(super) struct DirectStreamTimedResponse {
    pub(super) body: Vec<u8>,
    pub(super) send_ms: u32,
    pub(super) response_read_ms: u32,
    pub(super) total_ms: u32,
    pub(super) body_cadence: DirectStreamBodyCadence,
}

pub(super) fn request_raw_timed_direct_stream(
    method: &Method,
    url: &str,
    body: &[u8],
    token: Option<&str>,
    timeout_s: f64,
    burst_bytes: usize,
    tcp_nodelay: bool,
) -> Result<DirectStreamTimedResponse> {
    let (addr, host_header, target) = parse_url_for_http(url)?;
    let timeout = Duration::from_secs_f64(timeout_s.max(0.1));
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .with_context(|| format!("{method} {url} send failed"))?;
    stream
        .set_nodelay(tcp_nodelay)
        .with_context(|| format!("{method} {url} send failed"))?;
    stream
        .set_read_timeout(Some(timeout))
        .with_context(|| format!("{method} {url} send failed"))?;
    stream
        .set_write_timeout(Some(timeout))
        .with_context(|| format!("{method} {url} send failed"))?;

    let request_started_at = Instant::now();
    let send_started_at = request_started_at;
    let header = build_request_header(method, &host_header, &target, body.len(), token);
    stream
        .write_all(header.as_bytes())
        .with_context(|| format!("{method} {url} send failed"))?;

    let mut cadence = DirectStreamBodyCadence::default();
    let mut last_write_completed_at: Option<Instant> = None;
    let write_chunk_bytes = burst_bytes.max(1);
    for chunk in body.chunks(write_chunk_bytes) {
        if let Some(previous_completed_at) = last_write_completed_at {
            let gap_ms = elapsed_ms_u32(previous_completed_at);
            cadence.read_gap_ms_total = cadence.read_gap_ms_total.saturating_add(gap_ms);
            cadence.read_gap_ms_max = cadence.read_gap_ms_max.max(gap_ms);
            if gap_ms >= 10 {
                cadence.read_gap_over_10ms = cadence.read_gap_over_10ms.saturating_add(1);
            }
            if gap_ms >= 50 {
                cadence.read_gap_over_50ms = cadence.read_gap_over_50ms.saturating_add(1);
            }
        }
        stream
            .write_all(chunk)
            .with_context(|| format!("{method} {url} send failed"))?;
        cadence.read_calls = cadence.read_calls.saturating_add(1);
        cadence.read_bytes = cadence.read_bytes.saturating_add(chunk.len());
        if chunk.len() < write_chunk_bytes {
            cadence.short_reads = cadence.short_reads.saturating_add(1);
        }
        last_write_completed_at = Some(Instant::now());
    }
    stream
        .flush()
        .with_context(|| format!("{method} {url} send failed"))?;
    let _ = stream.shutdown(Shutdown::Write);
    let send_ms = elapsed_ms_u32(send_started_at);

    let response_started_at = Instant::now();
    let response = read_http_response(&mut stream).context("failed reading response body")?;
    let response_read_ms = elapsed_ms_u32(response_started_at);
    if !(200..300).contains(&response.status_code) {
        return Err(anyhow!(
            "{method} {url} failed: {} {}",
            response.status_code,
            String::from_utf8_lossy(&response.body)
        ));
    }

    Ok(DirectStreamTimedResponse {
        body: response.body,
        send_ms,
        response_read_ms,
        total_ms: elapsed_ms_u32(request_started_at),
        body_cadence: cadence,
    })
}

fn parse_url_for_http(url: &str) -> Result<(std::net::SocketAddr, String, String)> {
    let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid url: {url}"))?;
    if parsed.scheme() != "http" {
        return Err(anyhow!("direct stream mode only supports http URLs"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("missing host in url: {url}"))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow!("missing port in url: {url}"))?;
    let mut addrs = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed resolving {host}:{port}"))?;
    let addr = addrs
        .next()
        .ok_or_else(|| anyhow!("no socket addresses for {host}:{port}"))?;
    let path = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    let target = match parsed.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };
    Ok((addr, format!("{host}:{port}"), target))
}

fn build_request_header(
    method: &Method,
    host_header: &str,
    target: &str,
    content_length: usize,
    token: Option<&str>,
) -> String {
    let mut out = String::with_capacity(192);
    out.push_str(method.as_str());
    out.push(' ');
    out.push_str(target);
    out.push_str(" HTTP/1.1\r\nHost: ");
    out.push_str(host_header);
    out.push_str("\r\nConnection: close\r\nContent-Length: ");
    out.push_str(&content_length.to_string());
    out.push_str("\r\n");
    if let Some(token) = token {
        out.push_str("x-upload-token: ");
        out.push_str(token);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out
}

struct ParsedHttpResponse {
    status_code: u16,
    body: Vec<u8>,
}

fn read_http_response(stream: &mut TcpStream) -> Result<ParsedHttpResponse> {
    let mut raw = Vec::<u8>::with_capacity(HTTP_READ_BUFFER_BYTES);
    let mut scratch = [0u8; HTTP_READ_BUFFER_BYTES];

    let header_end = loop {
        let n = stream.read(&mut scratch)?;
        if n == 0 {
            return Err(anyhow!("eof before response headers"));
        }
        raw.extend_from_slice(&scratch[..n]);
        if raw.len() > HTTP_HEADER_MAX_BYTES {
            return Err(anyhow!(
                "response headers exceed {} bytes",
                HTTP_HEADER_MAX_BYTES
            ));
        }
        if let Some(end) = find_bytes(&raw, HTTP_HEADER_END) {
            break end;
        }
    };

    let header_blob = &raw[..header_end];
    let header_text = core::str::from_utf8(header_blob).context("response headers not utf8")?;
    let status_code = parse_status_code(header_text)?;
    let content_length = parse_content_length(header_text)?;
    let body_offset = header_end + HTTP_HEADER_END.len();
    if let Some(length) = content_length {
        while raw.len().saturating_sub(body_offset) < length {
            let n = stream.read(&mut scratch)?;
            if n == 0 {
                return Err(anyhow!(
                    "eof before full response body (wanted {} bytes, have {})",
                    length,
                    raw.len().saturating_sub(body_offset)
                ));
            }
            raw.extend_from_slice(&scratch[..n]);
        }
        return Ok(ParsedHttpResponse {
            status_code,
            body: raw[body_offset..body_offset + length].to_vec(),
        });
    }

    loop {
        match stream.read(&mut scratch) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&scratch[..n]),
            Err(err)
                if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(ParsedHttpResponse {
        status_code,
        body: raw[body_offset..].to_vec(),
    })
}

fn parse_status_code(headers: &str) -> Result<u16> {
    let status_line = headers
        .lines()
        .next()
        .ok_or_else(|| anyhow!("missing status line"))?;
    let code = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| anyhow!("missing status code"))?;
    code.parse::<u16>()
        .with_context(|| format!("invalid status code: {code}"))
}

fn parse_content_length(headers: &str) -> Result<Option<usize>> {
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let parsed = value
                .trim()
                .parse::<usize>()
                .with_context(|| format!("invalid Content-Length: {}", value.trim()))?;
            return Ok(Some(parsed));
        }
    }
    Ok(None)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u128 {
        u32::MAX
    } else {
        elapsed as u32
    }
}
