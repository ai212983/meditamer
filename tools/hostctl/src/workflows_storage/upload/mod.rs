use std::path::PathBuf;

mod client;
mod pathing;
mod run;
mod transfer;

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

pub use run::{run_upload, upload_file_direct_fast};
