use std::io::ErrorKind;

pub(super) fn elapsed_ms_u32(started_at: std::time::Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u128 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn compact_diag_text(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ReqwestErrorFlags {
    pub(super) seen: bool,
    pub(super) timeout: bool,
    pub(super) connect: bool,
    pub(super) request: bool,
    pub(super) body: bool,
}

impl ReqwestErrorFlags {
    pub(super) fn transient(self) -> bool {
        self.timeout || self.connect || self.request || self.body
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct IoErrorFlags {
    pub(super) connection_reset: bool,
    pub(super) broken_pipe: bool,
    pub(super) connection_aborted: bool,
    pub(super) timed_out: bool,
    pub(super) connection_refused: bool,
    pub(super) not_connected: bool,
}

impl IoErrorFlags {
    pub(super) fn transient(self) -> bool {
        self.connection_reset
            || self.broken_pipe
            || self.connection_aborted
            || self.timed_out
            || self.connection_refused
            || self.not_connected
    }
}

pub(super) fn inspect_reqwest_error_flags(err: &anyhow::Error) -> ReqwestErrorFlags {
    let mut flags = ReqwestErrorFlags::default();
    for cause in err.chain() {
        if let Some(req_err) = cause.downcast_ref::<reqwest::Error>() {
            flags.seen = true;
            flags.timeout |= req_err.is_timeout();
            flags.connect |= req_err.is_connect();
            flags.request |= req_err.is_request();
            flags.body |= req_err.is_body();
        }
    }
    flags
}

pub(super) fn inspect_io_error_flags(err: &anyhow::Error) -> IoErrorFlags {
    let mut flags = IoErrorFlags::default();
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            match io_err.kind() {
                ErrorKind::ConnectionReset => flags.connection_reset = true,
                ErrorKind::BrokenPipe => flags.broken_pipe = true,
                ErrorKind::ConnectionAborted => flags.connection_aborted = true,
                ErrorKind::TimedOut => flags.timed_out = true,
                ErrorKind::ConnectionRefused => flags.connection_refused = true,
                ErrorKind::NotConnected => flags.not_connected = true,
                _ => {}
            }
        }
    }
    flags
}

pub(super) fn format_error_chain(err: &anyhow::Error, max_causes: usize) -> String {
    let limit = max_causes.max(1);
    let mut out = String::new();
    for (idx, cause) in err.chain().enumerate() {
        if idx >= limit {
            out.push_str(" <- ...");
            break;
        }
        if idx > 0 {
            out.push_str(" <- ");
        }
        out.push_str(&compact_diag_text(&cause.to_string()));
    }
    out
}

pub(super) fn is_transport_reset_error(msg_lower: &str) -> bool {
    msg_lower.contains("connection reset")
        || msg_lower.contains("send failed")
        || msg_lower.contains("error sending request")
        || msg_lower.contains("connection aborted")
        || msg_lower.contains("broken pipe")
}
