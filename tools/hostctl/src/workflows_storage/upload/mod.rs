use std::path::PathBuf;

mod client;
mod direct_stream;
mod pathing;
mod run;
mod transfer;

#[derive(Clone, Copy, Debug)]
pub struct UploadRetryPolicy {
    pub sd_busy_total_retry_sec: f64,
    pub net_recovery_timeout_sec: f64,
    pub net_recovery_poll_sec: f64,
    pub net_recovery_consecutive_health_successes: u32,
}

#[derive(Clone, Debug)]
pub struct UploadOptions {
    pub host: String,
    pub port: u16,
    pub src: Option<PathBuf>,
    pub dst: String,
    pub timeout_sec: f64,
    pub rm: Vec<String>,
    pub token: Option<String>,
}

pub use run::{
    make_direct_upload_client, run_upload, stat_remote_file, upload_file_direct_fast_with_client,
    DirectUploadOptions,
};
