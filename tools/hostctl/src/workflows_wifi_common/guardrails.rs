use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use fs2::FileExt;

use crate::env_utils;

use super::NetPolicy;

const SCAN_ACTIVE_MIN_FLOOR_MS: u32 = 600;
const SCAN_ACTIVE_MAX_FLOOR_MS: u32 = 1_500;
const SCAN_PASSIVE_FLOOR_MS: u32 = 1_500;
const DISCOVERY_RECOVER_SETTLE_FLOOR_MS: u32 = 6_000;

pub struct PortRunLock {
    file: File,
}

impl Drop for PortRunLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub fn enforce_log_path_policy(log_path: &Path) -> Result<()> {
    let allow_append = env_utils::parse_env_bool01("HOSTCTL_NET_ALLOW_LOG_APPEND", false)?;
    enforce_log_path_policy_with_allow(log_path, allow_append)
}

pub fn enforce_policy_floors(
    policy: NetPolicy,
    discovery_recover_settle_ms: Option<u32>,
) -> Result<()> {
    let enforce = env_utils::parse_env_bool01("HOSTCTL_NET_ENFORCE_POLICY_FLOORS", true)?;
    enforce_policy_floors_with_toggle(policy, discovery_recover_settle_ms, enforce)
}

pub fn acquire_port_lock(port: &str) -> Result<PortRunLock> {
    let lock_path = std::env::var("HOSTCTL_NET_LOCK_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_lock_path(port));
    let wait_sec = env_utils::parse_env_f64("HOSTCTL_NET_LOCK_WAIT_SEC", 0.0)?.max(0.0);
    acquire_port_lock_with(port, lock_path, wait_sec)
}

fn enforce_log_path_policy_with_allow(log_path: &Path, allow_append: bool) -> Result<()> {
    if !allow_append && log_path.exists() {
        return Err(anyhow!(
            "refusing to reuse existing HOSTCTL_NET_LOG_PATH={} (set HOSTCTL_NET_ALLOW_LOG_APPEND=1 to allow append explicitly)",
            log_path.display()
        ));
    }
    Ok(())
}

fn enforce_policy_floors_with_toggle(
    policy: NetPolicy,
    discovery_recover_settle_ms: Option<u32>,
    enforce: bool,
) -> Result<()> {
    if !enforce {
        return Ok(());
    }

    let mut violations = Vec::new();
    if policy.scan_active_min_ms < SCAN_ACTIVE_MIN_FLOOR_MS {
        violations.push(format!(
            "scan_active_min_ms={} below floor {}",
            policy.scan_active_min_ms, SCAN_ACTIVE_MIN_FLOOR_MS
        ));
    }
    if policy.scan_active_max_ms < SCAN_ACTIVE_MAX_FLOOR_MS {
        violations.push(format!(
            "scan_active_max_ms={} below floor {}",
            policy.scan_active_max_ms, SCAN_ACTIVE_MAX_FLOOR_MS
        ));
    }
    if policy.scan_passive_ms < SCAN_PASSIVE_FLOOR_MS {
        violations.push(format!(
            "scan_passive_ms={} below floor {}",
            policy.scan_passive_ms, SCAN_PASSIVE_FLOOR_MS
        ));
    }
    if let Some(recover_settle_ms) = discovery_recover_settle_ms {
        if recover_settle_ms < DISCOVERY_RECOVER_SETTLE_FLOOR_MS {
            violations.push(format!(
                "recover_settle_ms={} below floor {}",
                recover_settle_ms, DISCOVERY_RECOVER_SETTLE_FLOOR_MS
            ));
        }
    }

    if !violations.is_empty() {
        return Err(anyhow!(
            "wifi guardrail policy floor violation(s): {}",
            violations.join("; ")
        ));
    }

    Ok(())
}

fn acquire_port_lock_with(port: &str, lock_path: PathBuf, wait_sec: f64) -> Result<PortRunLock> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed creating lock directory {}", parent.display()))?;
    }

    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed opening lock file {}", lock_path.display()))?;

    let deadline = Instant::now() + Duration::from_secs_f64(wait_sec);
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(PortRunLock { file }),
            Err(err) => {
                if Instant::now() >= deadline {
                    return Err(anyhow!(
                        "failed to acquire HOSTCTL_NET lock for port={} lock_path={} wait_sec={} ({err})",
                        port,
                        lock_path.display(),
                        wait_sec
                    ));
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn default_lock_path(port: &str) -> PathBuf {
    default_repo_root()
        .join("logs")
        .join("locks")
        .join(format!("wifi_{}.lock", sanitize_port_for_lock_name(port)))
}

fn default_repo_root() -> PathBuf {
    // tools/hostctl/src/... -> repo root is three levels up from CARGO_MANIFEST_DIR.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn sanitize_port_for_lock_name(port: &str) -> String {
    let mut out = String::with_capacity(port.len());
    for ch in port.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        acquire_port_lock_with, enforce_log_path_policy_with_allow,
        enforce_policy_floors_with_toggle,
    };
    use crate::workflows_wifi_common::NetPolicy;

    #[test]
    fn log_path_policy_rejects_existing_without_append() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("wifi.log");
        std::fs::write(&path, "existing").expect("write");
        let err = enforce_log_path_policy_with_allow(&path, false).expect_err("must fail");
        assert!(err.to_string().contains("HOSTCTL_NET_LOG_PATH"));
    }

    #[test]
    fn log_path_policy_allows_existing_with_append() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("wifi.log");
        std::fs::write(&path, "existing").expect("write");
        enforce_log_path_policy_with_allow(&path, true).expect("append allowed");
    }

    #[test]
    fn policy_floor_checks_reject_underflow_values() {
        let policy = NetPolicy {
            scan_active_min_ms: 500,
            ..NetPolicy::default()
        };
        let err =
            enforce_policy_floors_with_toggle(policy, Some(2_000), true).expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("scan_active_min_ms=500"));
        assert!(msg.contains("recover_settle_ms=2000"));
    }

    #[test]
    fn policy_floor_checks_can_be_disabled() {
        let policy = NetPolicy {
            scan_passive_ms: 10,
            ..NetPolicy::default()
        };
        enforce_policy_floors_with_toggle(policy, Some(1_000), false).expect("disabled");
    }

    #[test]
    fn port_lock_rejects_second_runner_when_wait_is_zero() {
        let temp = tempdir().expect("tempdir");
        let lock_path = temp.path().join("lock").join("wifi.lock");
        let _first = acquire_port_lock_with("ttyUSB0", lock_path.clone(), 0.0).expect("first lock");
        let err = match acquire_port_lock_with("ttyUSB0", lock_path, 0.0) {
            Ok(_) => panic!("expected lock contention failure"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("failed to acquire HOSTCTL_NET lock"));
    }

    #[test]
    fn port_lock_can_be_acquired_after_drop() {
        let temp = tempdir().expect("tempdir");
        let lock_path = temp.path().join("wifi.lock");
        {
            let _first =
                acquire_port_lock_with("ttyUSB0", lock_path.clone(), 0.0).expect("first lock");
        }
        let _second = acquire_port_lock_with("ttyUSB0", lock_path, 0.0).expect("second lock");
    }
}
