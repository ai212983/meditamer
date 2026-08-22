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

use workflows::artifacts::{run_artifacts_inventory, run_artifacts_prune, ArtifactsPruneOptions};
use workflows::ble_phase1d::BlePhase1dOptions;
use workflows::ble_phase1s::BlePhase1sOptions;
use workflows::flash_capture::{run_flash_capture, CaptureMode, FlashCaptureOptions, FlashMode};
use workflows::runtime_modes::RuntimeModesSmokeOptions;
use workflows::sdcard::{SdcardHwOptions, SdcardSuite};
use workflows::serial::{RepaintOptions, TimeSetOptions, TimeStatusOptions};
use workflows::signing_key::firmware_public_key_hex;
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
    Artifacts(ArtifactsArgs),
    FlashCapture(FlashCaptureArgs),
    FirmwareKey(FirmwareKeyArgs),
    Repaint(RepaintArgs),
    SingleProductionBundleBuild(SingleProductionBundleBuildArgs),
    SingleProductionBundleInspect(SingleProductionBundleInspectArgs),
    SingleProductionFlash(SingleProductionFlashArgs),
    SingleProductionSdPush(SingleProductionSdPushArgs),
    TimeSet(TimeSetArgs),
    TimeStatus(TimeStatusArgs),
    Upload(UploadArgs),
    Test(TestArgs),
}

#[derive(Debug, Args)]
struct ArtifactsArgs {
    #[command(subcommand)]
    command: ArtifactsSubcommand,
}

#[derive(Debug, Subcommand)]
enum ArtifactsSubcommand {
    Inventory(ArtifactsInventoryArgs),
    Prune(ArtifactsPruneArgs),
}

/// Read-only totals, classifier output, and retention state for `logs/`.
#[derive(Debug, Args)]
struct ArtifactsInventoryArgs {}

/// Recognized flash-payload thinning. Defaults to a dry run; `--apply`
/// removes eligible payloads and writes one timestamped prune report.
/// `--runs` also expires whole run units and standalone logs past their
/// outcome-based age.
#[derive(Debug, Args)]
struct ArtifactsPruneArgs {
    #[arg(long)]
    apply: bool,
    #[arg(long)]
    ignore_age: bool,
    #[arg(long)]
    runs: bool,
}

#[derive(Debug, Args)]
struct FirmwareKeyArgs {
    #[arg(long)]
    key: PathBuf,
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
    /// Disables the automatic post-capture wall-clock sync, overriding
    /// `FLASH_SET_TIME_AFTER_FLASH` outright.
    #[arg(long)]
    no_time_sync: bool,
}

#[derive(Debug, Args)]
struct RepaintArgs {
    #[arg(long)]
    command: Option<String>,
}

/// Builds and signs an ADR-0014 bundle (header + firmware payload) from a
/// release image, ready to stage on SD via `single-production-sd-push` or a
/// real delivery transport.
#[derive(Debug, Args)]
struct SingleProductionBundleBuildArgs {
    /// The production release image to wrap (e.g. an `espflash save-image`
    /// output).
    #[arg(long)]
    firmware: PathBuf,
    /// Signing key: 32 raw bytes or 64 hex characters (same format
    /// `firmware-key` already accepts).
    #[arg(long)]
    key: PathBuf,
    #[arg(long, default_value_t = 1)]
    target_id: u16,
    #[arg(long, default_value_t = 1)]
    layout_id: u16,
    #[arg(long)]
    build_id: String,
    #[arg(long)]
    out: PathBuf,
}

/// Parses a bundle's header and reports its fields, without touching a
/// device — for confirming a bundle built elsewhere is well-formed before
/// staging it.
#[derive(Debug, Args)]
struct SingleProductionBundleInspectArgs {
    bundle: PathBuf,
    /// 64 hex characters; verifies the signature if given (matches
    /// `firmware-key`'s output format).
    #[arg(long)]
    public_key: Option<String>,
}

/// Pushes a signed bundle to a board's SD card over serial (ADR-0014 Phase
/// 4) — bench/qualification only, since the device's SD card is not
/// otherwise reachable without disassembly. The board must already be
/// running the `sd-qual-push` updater build variant
/// (`CARGO_FEATURES=sd-qual-push`); see src/updater/sd_push.rs.
#[derive(Debug, Args)]
struct SingleProductionSdPushArgs {
    #[arg(long)]
    port: Option<String>,
    bundle: PathBuf,
    #[arg(long)]
    output: Option<PathBuf>,
}

/// Complete USB flash for the single-production layout (ADR-0014 Phase 2):
/// bootloader + partition table + factory (updater) image + production
/// (`ota_0`) image + a freshly constructed initial `otadata`, in one
/// `esptool.py write_flash`. Build the bootloader/partition table first
/// with `scripts/build/single_production_bootloader.sh`.
#[derive(Debug, Args)]
struct SingleProductionFlashArgs {
    #[arg(long)]
    port: String,
    #[arg(long, default_value_t = 460_800)]
    baud: u32,
    /// The factory-updater release image (`target/xtensa-esp32-none-elf/release/updater`,
    /// via `espflash save-image`).
    #[arg(long)]
    factory: PathBuf,
    /// The production (`ota_0`) release image.
    #[arg(long)]
    production: PathBuf,
    /// Defaults to `target/single-production-bootloader/bootloader/bootloader.bin`.
    #[arg(long)]
    bootloader: Option<PathBuf>,
    /// Defaults to `target/single-production-bootloader/partition_table/partition-table.bin`.
    #[arg(long)]
    partition_table: Option<PathBuf>,
}

/// Synchronizes the device's PCF85063A wall clock to the host's current UTC
/// time and fixed local offset, then verifies the readback.
#[derive(Debug, Args)]
struct TimeSetArgs {}

/// Reports the device's current wall-clock validity via `TIMEGET`. Exits
/// successfully for either `TIMEGET OK` form (`valid=on` or `valid=off`);
/// only a transport error, parse error, or `TIMEGET ERR` is a failure.
#[derive(Debug, Args)]
struct TimeStatusArgs {}

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
    BlePhase1s(BlePhase1sArgs),
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
struct BlePhase1sArgs {
    #[arg(long)]
    artifacts: PathBuf,
    #[arg(long)]
    board_id: String,
    #[arg(long, default_value_t = 20)]
    cycles: u32,
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

fn run(cli: Cli) -> Result<()> {
    std::env::set_current_dir(workflows::common::repo_root())
        .context("set hostctl runtime working directory to repository root")?;
    let mut logger = Logger::from_env()?;

    match cli.command {
        Commands::Artifacts(args) => match args.command {
            ArtifactsSubcommand::Inventory(_) => run_artifacts_inventory(&mut logger),
            ArtifactsSubcommand::Prune(prune_args) => run_artifacts_prune(
                &mut logger,
                ArtifactsPruneOptions {
                    apply: prune_args.apply,
                    ignore_age: prune_args.ignore_age,
                    runs: prune_args.runs,
                },
            ),
        },
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
                no_time_sync: args.no_time_sync,
            },
        ),
        Commands::FirmwareKey(args) => {
            println!(
                "MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX={}",
                firmware_public_key_hex(&args.key)?
            );
            Ok(())
        }
        Commands::Repaint(args) => workflows::serial::run_repaint(
            &mut logger,
            RepaintOptions {
                command: args.command,
            },
        ),
        Commands::SingleProductionBundleBuild(args) => {
            let built = workflows::single_production::bundle::build_and_sign(
                &args.firmware,
                &args.key,
                args.target_id,
                args.layout_id,
                &args.build_id,
                &args.out,
            )?;
            println!("bundle_bytes={}", built.bundle_bytes);
            println!("firmware_len={}", built.firmware_len);
            println!(
                "firmware_digest={}",
                workflows::single_production::bundle::hex(&built.firmware_digest)
            );
            println!("build_id={}", built.build_id);
            println!("public_key_hex={}", built.public_key_hex);
            println!("out={}", args.out.display());
            Ok(())
        }
        Commands::SingleProductionBundleInspect(args) => {
            let inspected = workflows::single_production::bundle::inspect(
                &args.bundle,
                args.public_key.as_deref(),
            )?;
            println!("target_id={}", inspected.target_id);
            println!("layout_id={}", inspected.layout_id);
            println!("build_id={}", inspected.build_id);
            println!("firmware_len={}", inspected.firmware_len);
            println!(
                "firmware_digest={}",
                workflows::single_production::bundle::hex(&inspected.firmware_digest)
            );
            println!(
                "signature_valid={}",
                inspected
                    .signature_valid
                    .map_or("not_checked".to_string(), |v| v.to_string())
            );
            println!(
                "payload_digest_matches={}",
                inspected
                    .payload_digest_matches
                    .map_or("truncated".to_string(), |v| v.to_string())
            );
            Ok(())
        }
        Commands::SingleProductionSdPush(args) => workflows::single_production::sd_push::run_sd_push(
            &mut logger,
            workflows::single_production::sd_push::SdPushOptions {
                port: args.port,
                bundle_path: args.bundle,
                output: args.output,
            },
        ),
        Commands::SingleProductionFlash(args) => {
            let idf_env = idf_env::bootstrap_idf_env(None, None)?;
            let repo_root = workflows::common::repo_root();
            let build_dir = repo_root.join("target/single-production-bootloader");
            let bootloader = args
                .bootloader
                .unwrap_or_else(|| build_dir.join("bootloader/bootloader.bin"));
            let partition_table = args
                .partition_table
                .unwrap_or_else(|| build_dir.join("partition_table/partition-table.bin"));
            let otadata_scratch = build_dir.join("otadata-initial.bin");
            workflows::single_production::flash::run_complete_flash(
                workflows::single_production::flash::CompleteFlashInputs {
                    idf_env: &idf_env,
                    port: &args.port,
                    flash_baud: args.baud,
                    bootloader_bin: &bootloader,
                    partition_table_bin: &partition_table,
                    factory_bin: &args.factory,
                    production_bin: &args.production,
                    otadata_scratch_path: &otadata_scratch,
                },
            )
        }
        Commands::TimeSet(_args) => workflows::serial::run_timeset(&mut logger, TimeSetOptions {}),
        Commands::TimeStatus(_args) => {
            workflows::serial::run_timestatus(&mut logger, TimeStatusOptions {})
        }
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
            TestSubcommand::BlePhase1s(test_args) => workflows::ble_phase1s::run_ble_phase1s(
                &mut logger,
                BlePhase1sOptions {
                    artifacts: test_args.artifacts,
                    board_id: test_args.board_id,
                    cycles: test_args.cycles,
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
