mod boot_gate;
mod host_wifi;
mod runtime_core;
mod runtime_upload;
mod wait_ready;

use std::{fs, path::PathBuf, time::Instant};

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::{
    env_utils,
    logging::{ensure_parent_dir, Logger},
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
    workflows::wifi::common::{
        acquire_port_lock, enforce_log_path_policy, enforce_policy_floors, preflight,
        MemDiagSummary, NetPolicy, PanicSignal,
    },
};

pub(crate) use host_wifi::ensure_host_wifi_association;

#[derive(Clone, Debug)]
pub struct WifiAcceptanceOptions {
    pub output_path: Option<PathBuf>,
    pub target: WifiAcceptanceTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WifiAcceptanceTarget {
    Inkplate,
    S3,
}

impl std::str::FromStr for WifiAcceptanceTarget {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "inkplate" => Ok(Self::Inkplate),
            "s3" | "waveshare" | "medinote" => Ok(Self::S3),
            other => bail!("unknown Wi-Fi acceptance target {other:?}; expected inkplate or s3"),
        }
    }
}

struct WifiAcceptanceRuntime<'a> {
    logger: &'a mut Logger,
    console: SerialConsole,
    payload_path: PathBuf,
    remote_root: String,
    ssid: String,
    password: String,
    token: Option<String>,
    policy: NetPolicy,
    cycles: u32,
    operation_retries: u32,
    connect_samples: Vec<f64>,
    listen_samples: Vec<f64>,
    upload_samples: Vec<f64>,
    throughput_samples: Vec<f64>,
    started: Instant,
    mem_diag: MemDiagSummary,
    mem_read_mark: usize,
    panic_monitoring_enabled: bool,
    panic_first: Option<PanicSignal>,
    req_read_body_reset_max_delta: u32,
    req_read_body_reset_baseline: Option<u32>,
    upload_client: Option<reqwest::blocking::Client>,
    reuse_upload_client: bool,
    target: WifiAcceptanceTarget,
    discovery_mark: usize,
    s3_uptime_ms: Option<u32>,
    s3_start_ack_samples: Vec<f64>,
}

pub fn run_wifi_acceptance(logger: &mut Logger, opts: WifiAcceptanceOptions) -> Result<()> {
    let port = std::env::var("HOSTCTL_NET_PORT")
        .context("HOSTCTL_NET_PORT must be set (hard-cut net workflow)")?;
    let baud = std::env::var("HOSTCTL_NET_BAUD")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(115200);
    let ssid = std::env::var("HOSTCTL_NET_SSID")
        .context("HOSTCTL_NET_SSID must be set (hard-cut net workflow)")?;
    let password = std::env::var("HOSTCTL_NET_PASSWORD").unwrap_or_default();
    let policy_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scenarios/wifi-policy.default.json")
        .display()
        .to_string();
    let skip_host_wifi_check =
        env_utils::parse_env_bool01("HOSTCTL_NET_SKIP_HOST_WIFI_CHECK", false)?;
    if !skip_host_wifi_check {
        ensure_host_wifi_association(&ssid)?;
    }
    let log_path = opts.output_path.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOSTCTL_NET_LOG_PATH").unwrap_or_else(|_| {
            format!(
                "logs/wifi_acceptance_{}.log",
                chrono::Local::now().format("%Y%m%d_%H%M%S")
            )
        }))
    });

    let policy_raw = fs::read_to_string(&policy_path)
        .with_context(|| format!("failed reading wifi policy template: {policy_path}"))?;
    let policy = serde_json::from_str::<NetPolicy>(&policy_raw)
        .context("invalid wifi policy template JSON")?;
    enforce_policy_floors(policy, None)?;
    enforce_log_path_policy(&log_path)?;
    ensure_parent_dir(&log_path)?;
    let _port_lock = acquire_port_lock(&port)?;

    // Deliberately use the resetting open: S3 acceptance observes a fresh boot
    // and its persisted configuration before cycling the radio.
    let mut console = SerialConsole::open(&port, baud, Some(&log_path))?;
    preflight(&mut console)?;

    let cycles = env_utils::parse_env_u32("HOSTCTL_NET_CYCLES", 3)?.max(1);
    // Acceptance-internal knobs, formerly env-tunable; hard-coded (see
    // docs/archive/host-tooling/hostctl-env-audit-completed-2026-09-07.md category 3).
    let operation_retries = 3u32;
    let req_read_body_reset_max_delta = 0u32;
    // Decided blackout-era A/B knob (HOSTCTL_NET_REUSE_UPLOAD_CLIENT): off.
    let reuse_upload_client = false;
    let workflow = load_workflow(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml"),
    )?;

    let payload_path = PathBuf::from("/tmp/net_acceptance_payload.bin");
    let remote_root = "/assets".to_string();
    let token = std::env::var("HOSTCTL_UPLOAD_TOKEN").ok();

    let mut runtime = WifiAcceptanceRuntime {
        logger,
        console,
        payload_path,
        remote_root,
        ssid,
        password,
        token,
        policy,
        cycles,
        operation_retries,
        connect_samples: Vec::new(),
        listen_samples: Vec::new(),
        upload_samples: Vec::new(),
        throughput_samples: Vec::new(),
        started: Instant::now(),
        mem_diag: MemDiagSummary::default(),
        mem_read_mark: 0,
        panic_monitoring_enabled: false,
        panic_first: None,
        req_read_body_reset_max_delta,
        req_read_body_reset_baseline: None,
        upload_client: None,
        reuse_upload_client,
        target: opts.target,
        discovery_mark: 0,
        s3_uptime_ms: None,
        s3_start_ack_samples: Vec::new(),
    };
    execute_workflow(
        &workflow,
        &mut runtime,
        &json!({"target_s3": opts.target == WifiAcceptanceTarget::S3}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyhow::{bail, Result};
    use serde_json::{json, Value};

    use crate::{
        scenarios::{execute_workflow, load_workflow, WorkflowRuntime},
        serial_console::AckStatus,
    };

    use super::runtime_core::set_discovery_log_domain;
    use super::WifiAcceptanceTarget;

    #[test]
    fn acceptance_target_parser_accepts_s3_aliases_and_rejects_unknown() {
        assert_eq!(
            "s3".parse::<WifiAcceptanceTarget>().unwrap(),
            WifiAcceptanceTarget::S3
        );
        assert_eq!(
            "waveshare".parse::<WifiAcceptanceTarget>().unwrap(),
            WifiAcceptanceTarget::S3
        );
        assert_eq!(
            "inkplate".parse::<WifiAcceptanceTarget>().unwrap(),
            WifiAcceptanceTarget::Inkplate
        );
        assert!("other".parse::<WifiAcceptanceTarget>().is_err());
    }

    #[test]
    fn s3_workflow_requires_discovery_upload_and_health_for_each_cycle() -> Result<()> {
        struct Runtime {
            actions: Vec<String>,
        }
        impl WorkflowRuntime for Runtime {
            fn invoke(&mut self, action: &str, _: &Value, _: &mut Value) -> Result<()> {
                self.actions.push(action.into());
                assert!(!matches!(
                    action,
                    "wait_runtime_ready"
                        | "set_discovery_log_domain"
                        | "boot_discovery_gate"
                        | "assert_upload_metrics"
                        | "assert_runtime_health"
                ));
                Ok(())
            }
            fn invoke_with_result(
                &mut self,
                action: &str,
                args: &Value,
                ctx: &mut Value,
            ) -> Result<Option<Value>> {
                self.invoke(action, args, ctx)?;
                Ok(match action {
                    "start_run" => Some(json!({"cycle": 1, "cycles": 2, "operation_retries": 1})),
                    "init_wait_ready_recovery" => {
                        Some(json!({"net_wait_ready_loop_budget": 1, "ip": null}))
                    }
                    "net_wait_ready_once" => Some(json!({"ip": "192.0.2.1"})),
                    "init_upload_attempt" => Some(json!({"upload_attempt": 1})),
                    "net_upload_once" => Some(json!({"upload_done": true})),
                    _ => None,
                })
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml"),
        )?;
        let mut runtime = Runtime {
            actions: Vec::new(),
        };
        execute_workflow(&workflow, &mut runtime, &json!({"target_s3": true}))?;
        for action in [
            "s3_stop",
            "s3_configure",
            "s3_start",
            "s3_assert_discovery",
            "net_upload_once",
            "net_verify_once",
        ] {
            assert_eq!(
                runtime
                    .actions
                    .iter()
                    .filter(|value| value.as_str() == action)
                    .count(),
                2,
                "{action}"
            );
        }
        assert_eq!(
            runtime
                .actions
                .iter()
                .filter(|value| matches!(value.as_str(), "s3_runtime_health" | "s3_network_health"))
                .count(),
            4
        );
        Ok(())
    }

    #[test]
    fn wifi_acceptance_workflow_yaml_parses() -> Result<()> {
        let workflow_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml");
        load_workflow(&workflow_path).map(|_| ())
    }

    #[test]
    fn wifi_acceptance_requires_both_discovery_log_acks_before_boot_gate() -> Result<()> {
        struct Startup {
            actions: Vec<String>,
            commands: Vec<String>,
            acknowledgements: [AckStatus; 2],
        }
        impl WorkflowRuntime for Startup {
            fn invoke(&mut self, action: &str, args: &Value, _: &mut Value) -> Result<()> {
                self.actions.push(action.into());
                match action {
                    "prepare_payload" | "wait_runtime_ready" => Ok(()),
                    "set_discovery_log_domain" => set_discovery_log_domain(args, |command| {
                        let status = self.acknowledgements[self.commands.len()];
                        self.commands.push(command.into());
                        let line = match status {
                            AckStatus::Ok => Some("TELEMSET OK".into()),
                            AckStatus::Busy => Some("TELEMSET BUSY".into()),
                            AckStatus::Err => Some("TELEMSET ERR".into()),
                            AckStatus::None => None,
                        };
                        Ok((status, line))
                    }),
                    "boot_discovery_gate" => bail!("boot gate sentinel"),
                    _ => bail!("unexpected action {action}"),
                }
            }
        }
        let workflow = load_workflow(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/wifi-acceptance.sw.yaml"),
        )?;
        let failures = [AckStatus::Busy, AckStatus::Err, AckStatus::None];
        for acknowledgements in std::iter::once([AckStatus::Ok; 2]).chain(
            failures
                .into_iter()
                .flat_map(|failure| [[failure, AckStatus::Ok], [AckStatus::Ok, failure]]),
        ) {
            let mut runtime = Startup {
                actions: Vec::new(),
                commands: Vec::new(),
                acknowledgements,
            };
            let error = execute_workflow(&workflow, &mut runtime, &json!({"target_s3": false}))
                .unwrap_err();
            let expected_commands = ["TELEMSET WIFI ON", "TELEMSET REASSOC ON"];
            let successful = acknowledgements == [AckStatus::Ok; 2];
            let command_count = if acknowledgements[0] == AckStatus::Ok {
                2
            } else {
                1
            };
            assert_eq!(runtime.commands, expected_commands[..command_count]);
            let mut expected_actions = vec!["prepare_payload", "wait_runtime_ready"];
            expected_actions.extend(vec!["set_discovery_log_domain"; command_count]);
            if successful {
                expected_actions.push("boot_discovery_gate");
                assert!(format!("{error:#}").contains("boot gate sentinel"));
            } else {
                assert!(format!("{error:#}").contains("discovery logging prerequisite failed"));
            }
            assert_eq!(runtime.actions, expected_actions);
        }
        Ok(())
    }
}
