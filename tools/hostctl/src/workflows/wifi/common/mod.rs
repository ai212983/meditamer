mod context;
mod guardrails;
mod mem_diag;
mod net;
mod panic;
mod types;

pub use context::{ctx_get_string, ctx_get_u32};
pub use guardrails::{acquire_port_lock, enforce_log_path_policy, enforce_policy_floors};
pub use mem_diag::{fmt_min, MemDiagSummary};
#[cfg(test)]
pub use mem_diag::{parse_mem_diag_line, MemDiagKind};
pub use net::{
    is_ready, net_status_line_re, netcfg_set_payload, parse_net_status_line, parse_scan_done_count,
    preflight, query_net_status, query_net_status_line, wait_net_ack,
};
pub use panic::{detect_panic_signal, extract_context_window, PanicClass, PanicSignal};
pub use types::{NetPolicy, NetStatus};
