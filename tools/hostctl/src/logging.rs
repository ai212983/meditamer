use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde_json::json;

pub struct Logger {
    json_file: Option<File>,
}

impl Logger {
    pub fn from_env() -> Result<Self> {
        let path = std::env::var("HOSTCTL_LOG_JSON_PATH").ok();
        Self::new(path.map(PathBuf::from))
    }

    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let json_file = match path {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let file = OpenOptions::new().create(true).append(true).open(path)?;
                Some(file)
            }
            None => None,
        };
        Ok(Self { json_file })
    }

    pub fn info(&mut self, message: impl AsRef<str>) {
        println!("{}", message.as_ref());
        self.event("info", message.as_ref());
    }

    pub fn warn(&mut self, message: impl AsRef<str>) {
        eprintln!("{}", message.as_ref());
        self.event("warn", message.as_ref());
    }

    pub fn error(&mut self, message: impl AsRef<str>) {
        eprintln!("{}", message.as_ref());
        self.event("error", message.as_ref());
    }

    pub fn event(&mut self, level: &str, message: &str) {
        let Some(file) = &mut self.json_file else {
            return;
        };

        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let entry = json!({
            "ts_ms": ts_ms,
            "level": level,
            "msg": message,
        });

        let _ = writeln!(file, "{}", entry);
        let _ = file.flush();
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}
