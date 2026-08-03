#[path = "basic/common.rs"]
mod common;
#[cfg(feature = "asset-upload-http")]
#[path = "basic/network.rs"]
mod network;
#[path = "basic/state.rs"]
mod state;
#[path = "basic/telemetry.rs"]
mod telemetry;

pub(super) use common::{
    parse_allocator_alloc_probe_command, parse_allocator_status_command, parse_metrics_command,
    parse_metrics_net_command, parse_ping_command, parse_repaint_command, parse_scheduler_command,
    parse_sdprobe_command, parse_touch_sched_reset_command,
};
#[cfg(feature = "asset-upload-http")]
pub(super) use network::{
    parse_net_listener_command, parse_net_recover_command, parse_net_start_command,
    parse_net_status_command, parse_net_stop_command, parse_netcfg_get_command,
    parse_netcfg_set_command,
};
pub(super) use state::{
    parse_diag_get_command, parse_state_diag_command, parse_state_get_command,
    parse_state_set_command,
};
pub(super) use telemetry::{parse_telemetry_set_command, parse_telemetry_status_command};

#[cfg(all(test, not(target_os = "none")))]
#[path = "basic/tests.rs"]
mod tests;
