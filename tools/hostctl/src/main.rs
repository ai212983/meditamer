mod env_utils;
mod idf_env;
mod logging;
mod port_detect;
mod scenarios;
mod serial_console;
mod workflows;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use logging::Logger;

use workflows::ble_phase1d::BlePhase1dOptions;
use workflows::firmware_update::{
    firmware_public_key_hex, run_firmware_update, FirmwareUpdateOptions,
};
use workflows::flash_capture::{run_flash_capture, CaptureMode, FlashCaptureOptions, FlashMode};
use workflows::runtime_modes::RuntimeModesSmokeOptions;
use workflows::sdcard::{SdcardHwOptions, SdcardSuite};
use workflows::serial::RepaintOptions;
use workflows::troubleshoot::TroubleshootOptions;
use workflows::ui_lifecycle::UiLifecycleOptions;
use workflows::upload::UploadOptions;
use workflows::wifi::acceptance::WifiAcceptanceOptions;
use workflows::wifi::discovery::WifiDiscoveryDebugOptions;

#[derive(Debug, Parser)]
#[command(name = "hostctl")]
#[command(about = "Meditamer host instrumentation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    FlashCapture(FlashCaptureArgs),
    FirmwareKey(FirmwareKeyArgs),
    FirmwareUpdate(FirmwareUpdateArgs),
    Repaint(RepaintArgs),
    Upload(UploadArgs),
    Test(TestArgs),
}

#[derive(Debug, Args)]
struct FirmwareKeyArgs {
    #[arg(long)]
    key: PathBuf,
}

#[derive(Debug, Args)]
struct FirmwareUpdateArgs {
    #[arg(long)]
    image: PathBuf,
    #[arg(long)]
    key: PathBuf,
    #[arg(long)]
    port: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = false)]
    stage_only: bool,
}

#[derive(Debug, Args)]
struct FlashCaptureArgs {
    #[arg(long, default_value = "release")]
    profile: String,
    #[arg(long = "log")]
    output_path: Option<PathBuf>,
    #[arg(long)]
    port: Option<String>,
    #[arg(long, value_enum, default_value_t = FlashMode::Auto)]
    flash_mode: FlashMode,
    #[arg(long, value_enum, default_value_t = CaptureMode::Boot)]
    capture_mode: CaptureMode,
    #[arg(long)]
    image: Option<PathBuf>,
    #[arg(long)]
    flash_baud: Option<u32>,
    #[arg(long)]
    baud: Option<u32>,
    #[arg(long)]
    boot_window_ms: Option<u64>,
    #[arg(long)]
    idf_root: Option<PathBuf>,
    #[arg(long)]
    idf_tools_path: Option<PathBuf>,
    #[arg(long)]
    post_command: Option<String>,
    #[arg(long)]
    post_pattern: Option<String>,
    #[arg(long)]
    post_timeout_ms: Option<u64>,
}

#[derive(Debug, Args)]
struct RepaintArgs {
    #[arg(long)]
    command: Option<String>,
}

#[derive(Debug, Args)]
struct UploadArgs {
    #[arg(long)]
    host: String,
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long)]
    src: Option<PathBuf>,
    #[arg(long, default_value = "/assets")]
    dst: String,
    #[arg(long, default_value_t = 60.0)]
    timeout: f64,
    #[arg(long = "rm")]
    rm: Vec<String>,
    #[arg(long)]
    token: Option<String>,
}

#[derive(Debug, Args)]
struct TestArgs {
    #[command(subcommand)]
    test: TestSubcommand,
}

#[derive(Debug, Subcommand)]
enum TestSubcommand {
    BlePhase1d(BlePhase1dArgs),
    WifiAcceptance(WifiAcceptanceArgs),
    WifiDiscoveryDebug(WifiDiscoveryDebugArgs),
    RuntimeModesSmoke(RuntimeModesArgs),
    SdcardHw(SdcardArgs),
    SdcardBurstRegression(SdcardBurstArgs),
    Troubleshoot(TroubleshootArgs),
    UiLifecycle(UiLifecycleArgs),
}

#[derive(Debug, Args)]
struct BlePhase1dArgs {
    #[arg(long)]
    artifacts: PathBuf,
    #[arg(long)]
    board_id: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct WifiAcceptanceArgs {
    output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct WifiDiscoveryDebugArgs {
    output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct RuntimeModesArgs {
    #[arg(long, default_value = "full")]
    suite: String,
    output_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SdcardArgs {
    #[arg(long, default_value = "debug")]
    build_mode: String,
    #[arg(long, default_value = "all")]
    suite: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SdcardBurstArgs {
    #[arg(long, default_value = "debug")]
    build_mode: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct TroubleshootArgs {
    #[arg(long, default_value = "debug")]
    build_mode: String,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct UiLifecycleArgs {
    #[arg(long, default_value_t = 2)]
    cycles: u16,
    #[arg(long, default_value_t = 0)]
    max_baseline_drift_bytes: usize,
    #[arg(long)]
    output: Option<PathBuf>,
}

fn parse_suite(raw: &str) -> Result<SdcardSuite> {
    match raw {
        "all" => Ok(SdcardSuite::All),
        "baseline" => Ok(SdcardSuite::Baseline),
        "burst" => Ok(SdcardSuite::Burst),
        "failures" => Ok(SdcardSuite::Failures),
        "cutover" => Ok(SdcardSuite::Cutover),
        "no-card" => Ok(SdcardSuite::NoCard),
        _ => Err(anyhow::anyhow!(
            "Invalid suite `{raw}` (use all|baseline|burst|failures|cutover|no-card)"
        )),
    }
}

fn firmware_update_activate(cli_stage_only: bool, env_stage_only: bool) -> bool {
    !(cli_stage_only || env_stage_only)
}

fn run(cli: Cli) -> Result<()> {
    std::env::set_current_dir(workflows::common::repo_root())
        .context("set hostctl runtime working directory to repository root")?;
    let mut logger = Logger::from_env()?;

    match cli.command {
        Commands::FlashCapture(args) => run_flash_capture(
            &mut logger,
            FlashCaptureOptions {
                profile: args.profile,
                output_path: args.output_path,
                port: args.port,
                flash_mode: args.flash_mode,
                capture_mode: args.capture_mode,
                image: args.image,
                flash_baud: args.flash_baud,
                baud: args.baud,
                boot_window_ms: args.boot_window_ms,
                idf_root: args.idf_root,
                idf_tools_path: args.idf_tools_path,
                post_command: args.post_command,
                post_pattern: args.post_pattern,
                post_timeout_ms: args.post_timeout_ms,
            },
        ),
        Commands::FirmwareKey(args) => {
            println!(
                "MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX={}",
                firmware_public_key_hex(&args.key)?
            );
            Ok(())
        }
        Commands::FirmwareUpdate(args) => run_firmware_update(
            &mut logger,
            FirmwareUpdateOptions {
                image: args.image,
                key: args.key,
                port: args.port,
                output: args.output,
                activate: firmware_update_activate(
                    args.stage_only,
                    env_utils::parse_env_bool01("HOSTCTL_FIRMWARE_UPDATE_STAGE_ONLY", false)?,
                ),
            },
        ),
        Commands::Repaint(args) => workflows::serial::run_repaint(
            &mut logger,
            RepaintOptions {
                command: args.command,
            },
        ),
        Commands::Upload(args) => workflows::upload::run_upload(
            &mut logger,
            UploadOptions {
                host: args.host,
                port: args.port,
                src: args.src,
                dst: args.dst,
                timeout_sec: args.timeout,
                rm: args.rm,
                token: args.token,
            },
        ),
        Commands::Test(args) => match args.test {
            TestSubcommand::BlePhase1d(test_args) => workflows::ble_phase1d::run_ble_phase1d(
                &mut logger,
                BlePhase1dOptions {
                    artifacts: test_args.artifacts,
                    board_id: test_args.board_id,
                    output_path: test_args.output,
                },
            ),
            TestSubcommand::WifiAcceptance(test_args) => {
                workflows::wifi::acceptance::run_wifi_acceptance(
                    &mut logger,
                    WifiAcceptanceOptions {
                        output_path: test_args.output_path,
                    },
                )
            }
            TestSubcommand::WifiDiscoveryDebug(test_args) => {
                workflows::wifi::discovery::run_wifi_discovery_debug(
                    &mut logger,
                    WifiDiscoveryDebugOptions {
                        output_path: test_args.output_path,
                    },
                )
            }
            TestSubcommand::RuntimeModesSmoke(test_args) => {
                workflows::runtime_modes::run_runtime_modes_smoke(
                    &mut logger,
                    RuntimeModesSmokeOptions {
                        output_path: test_args.output_path,
                        suite: test_args.suite,
                    },
                )
            }
            TestSubcommand::SdcardHw(test_args) => workflows::sdcard::run_sdcard_hw(
                &mut logger,
                SdcardHwOptions {
                    build_mode: test_args.build_mode,
                    output_path: test_args.output,
                    suite: parse_suite(&test_args.suite)?,
                },
            ),
            TestSubcommand::SdcardBurstRegression(test_args) => {
                workflows::sdcard::run_sdcard_burst_regression(
                    &mut logger,
                    test_args.build_mode,
                    test_args.output,
                )
            }
            TestSubcommand::Troubleshoot(test_args) => workflows::troubleshoot::run_troubleshoot(
                &mut logger,
                TroubleshootOptions {
                    build_mode: test_args.build_mode,
                    output_path: test_args.output,
                },
            ),
            TestSubcommand::UiLifecycle(test_args) => workflows::ui_lifecycle::run_ui_lifecycle(
                &mut logger,
                UiLifecycleOptions {
                    cycles: test_args.cycles,
                    max_baseline_drift_bytes: test_args.max_baseline_drift_bytes,
                    output_path: test_args.output,
                },
            ),
        },
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("error: {err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::firmware_update_activate;

    #[test]
    fn either_stage_only_control_prevents_activation() {
        assert!(firmware_update_activate(false, false));
        assert!(!firmware_update_activate(true, false));
        assert!(!firmware_update_activate(false, true));
        assert!(!firmware_update_activate(true, true));
    }
}
