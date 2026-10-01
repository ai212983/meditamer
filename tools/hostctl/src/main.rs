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
use workflows::flash_capture::{
    run_flash_capture, BootTarget, CaptureMode, FlashCaptureOptions, FlashMode,
};
use workflows::observation_fixture::{FixtureProvider, ObservationFixtureOptions};
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
    /// Arm an action-triggered firmware trace and preserve its raw serial evidence.
    TraceCapture {
        #[arg(long)]
        output: PathBuf,
        /// Use a timed capture instead of waiting for an action and a STOP file.
        #[arg(long)]
        seconds: Option<u64>,
        /// Run the cross-core recorder integrity probe.
        #[arg(long, conflicts_with = "seconds")]
        self_test: bool,
    },
    /// Temporary, continuous-connection display thermal experiment.
    ThermalAba {
        #[arg(long)]
        output: PathBuf,
    },
    /// Short task/IRQ diagnostic capture with redraws paused.
    CpuProfile {
        #[arg(long)]
        output: PathBuf,
        /// Capture normal operation without pausing display updates.
        #[arg(long)]
        active_only: bool,
        /// Require a PONG before an in-flight metrics response finishes.
        #[arg(long, requires = "active_only")]
        probe_control: bool,
    },
    /// Qualify all scheduling profiles with finite active acquisition demand.
    MulticoreQualify {
        #[arg(long)]
        output: PathBuf,
        /// Wait for an operator START file and require touch coverage.
        #[arg(long)]
        physical_input: bool,
        /// Short three-profile screen; a pass is not full physical qualification.
        #[arg(long, requires = "physical_input")]
        short_screen: bool,
        /// One 36-second Upload diagnostic window with physical touch.
        #[arg(long, requires = "physical_input", conflicts_with = "short_screen")]
        upload_short: bool,
    },
    /// Repeat correlated full refreshes, optionally after each observed partial.
    PanelSoak {
        #[arg(long, default_value_t = 20)]
        cycles: u16,
        #[arg(long)]
        wait_partial: bool,
        #[arg(long)]
        output: PathBuf,
    },
    /// Observe the serial console without changing DTR or RTS.
    Monitor,
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
    /// Initial boot selection for a full flash. Factory preserves the
    /// updater-first recovery workflow; the day-to-day flash wrapper
    /// explicitly selects production.
    #[arg(long, value_enum, default_value_t = BootTarget::Factory)]
    boot_target: BootTarget,
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
    ObservationFixture(ObservationFixtureArgs),
    /// Verify Inkplate acquisitions during a bounded upload and full SD readback.
    ObservationUpload(workflows::storage::upload_probe::Options),
    SdUploadRecovery(workflows::storage::upload_probe::Options),
    ObservationSleep(workflows::observation_sleep::Options),
    AssetResidencyBaseline(AssetResidencyBaselineArgs),
    AssetResidencyReentry(AssetResidencyReentryArgs),
    AssetResidencyStress(AssetResidencyStressArgs),
    StackHeadroomWatch(AssetResidencyBaselineArgs),
    StackHeadroomWatchUploadOff(AssetResidencyBaselineArgs),
    StackHeadroomWatchUploadOn(AssetResidencyBaselineArgs),
}

#[derive(Debug, Args)]
struct AssetResidencyBaselineArgs {
    #[arg(long)]
    feature_label: String,
    #[arg(long)]
    build_label: String,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, default_value_t = 120)]
    marker_timeout_secs: u64,
}

#[derive(Debug, Args)]
struct AssetResidencyStressArgs {
    #[command(flatten)]
    baseline: AssetResidencyBaselineArgs,
    /// Number of 30-second clock liveness samples; zero targets association only.
    #[arg(long, default_value_t = 7)]
    hold_samples: u8,
}

#[derive(Debug, Args)]
struct AssetResidencyReentryArgs {
    #[command(flatten)]
    baseline: AssetResidencyBaselineArgs,
    /// Same-boot Mountain/Clock cycles; each toggles upload mode on and off.
    #[arg(long, default_value_t = 5)]
    cycles: u8,
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
    /// Target console contract (`inkplate` by default, or `s3`).
    #[arg(long, default_value = "inkplate")]
    target: String,
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

/// One expiring provider request; never retries or changes periodic demand.
#[derive(Debug, Args)]
struct ObservationFixtureArgs {
    #[arg(long, value_enum, default_value = "battery")]
    provider: FixtureProvider,
    #[arg(long)]
    request_id: Option<u64>,
    /// Replace periodic demand for a bounded session (60000..=300000ms cadence).
    #[arg(long)]
    period_ms: Option<u32>,
    /// Qualify Inkplate panel interruption and recovery separately from periodic expiry.
    #[arg(long)]
    panel_cycle: bool,
    /// Cancel once after this many samples instead of waiting for expiry.
    #[arg(long)]
    cancel_after_samples: Option<usize>,
    /// Firmware eligibility window, not expected conversion latency.
    #[arg(long, default_value_t = 300_000)]
    validity_ms: u32,
    /// Host response deadline; timing out does not cancel an admitted request.
    #[arg(long, default_value_t = 300_000)]
    timeout_ms: u64,
    /// Keep the same serial connection open after success for passive evidence.
    #[arg(long, default_value_t = 0)]
    observe_ms: u64,
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
        Commands::TraceCapture {
            output,
            seconds,
            self_test,
        } => workflows::trace_capture::run(output, seconds, self_test),
        Commands::ThermalAba { output } => workflows::thermal_aba::run(output),
        Commands::MulticoreQualify {
            output,
            physical_input,
            short_screen,
            upload_short,
        } => workflows::thermal_aba::run_multicore(
            output,
            physical_input,
            short_screen,
            upload_short,
        ),
        Commands::CpuProfile {
            output,
            active_only,
            probe_control,
        } => workflows::thermal_aba::run_profile(output, active_only, probe_control),
        Commands::PanelSoak {
            cycles,
            wait_partial,
            output,
        } => workflows::panel_soak::run(cycles, wait_partial, output),
        Commands::Monitor => workflows::passive_monitor::run(),
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
                boot_target: args.boot_target,
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
        Commands::SingleProductionSdPush(args) => {
            workflows::single_production::sd_push::run_sd_push(
                &mut logger,
                workflows::single_production::sd_push::SdPushOptions {
                    port: args.port,
                    bundle_path: args.bundle,
                    output: args.output,
                },
            )
        }
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
                token: args
                    .token
                    .or_else(|| std::env::var("HOSTCTL_UPLOAD_TOKEN").ok()),
            },
        ),
        Commands::Test(args) => run_test(&mut logger, args.test),
    }
}

fn run_test(logger: &mut Logger, command: TestSubcommand) -> Result<()> {
    match command {
        TestSubcommand::BlePhase1d(test_args) => workflows::ble_phase1d::run_ble_phase1d(
            logger,
            BlePhase1dOptions {
                artifacts: test_args.artifacts,
                board_id: test_args.board_id,
                output_path: test_args.output,
            },
        ),
        TestSubcommand::BlePhase1s(test_args) => workflows::ble_phase1s::run_ble_phase1s(
            logger,
            BlePhase1sOptions {
                artifacts: test_args.artifacts,
                board_id: test_args.board_id,
                cycles: test_args.cycles,
                output_path: test_args.output,
            },
        ),
        TestSubcommand::WifiAcceptance(test_args) => {
            workflows::wifi::acceptance::run_wifi_acceptance(
                logger,
                WifiAcceptanceOptions {
                    output_path: test_args.output_path,
                    target: test_args.target.parse()?,
                },
            )
        }
        TestSubcommand::WifiDiscoveryDebug(test_args) => {
            workflows::wifi::discovery::run_wifi_discovery_debug(
                logger,
                WifiDiscoveryDebugOptions {
                    output_path: test_args.output_path,
                },
            )
        }
        TestSubcommand::RuntimeModesSmoke(test_args) => {
            workflows::runtime_modes::run_runtime_modes_smoke(
                logger,
                RuntimeModesSmokeOptions {
                    output_path: test_args.output_path,
                    suite: test_args.suite,
                },
            )
        }
        TestSubcommand::SdcardHw(test_args) => workflows::sdcard::run_sdcard_hw(
            logger,
            SdcardHwOptions {
                build_mode: test_args.build_mode,
                output_path: test_args.output,
                suite: parse_suite(&test_args.suite)?,
            },
        ),
        TestSubcommand::SdcardBurstRegression(test_args) => {
            workflows::sdcard::run_sdcard_burst_regression(
                logger,
                test_args.build_mode,
                test_args.output,
            )
        }
        TestSubcommand::Troubleshoot(test_args) => workflows::troubleshoot::run_troubleshoot(
            logger,
            TroubleshootOptions {
                build_mode: test_args.build_mode,
                output_path: test_args.output,
            },
        ),
        TestSubcommand::UiLifecycle(test_args) => workflows::ui_lifecycle::run_ui_lifecycle(
            logger,
            UiLifecycleOptions {
                cycles: test_args.cycles,
                max_baseline_drift_bytes: test_args.max_baseline_drift_bytes,
                output_path: test_args.output,
            },
        ),
        TestSubcommand::ObservationSleep(options) => {
            workflows::observation_sleep::run(logger, options)
        }
        TestSubcommand::AssetResidencyBaseline(test_args) => {
            workflows::asset_residency::run_asset_residency_baseline(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.feature_label,
                    build_label: test_args.build_label,
                    output_path: test_args.output,
                    marker_timeout_secs: test_args.marker_timeout_secs,
                    hold_samples: 7,
                },
            )
        }
        TestSubcommand::AssetResidencyReentry(test_args) => {
            workflows::asset_residency::run_asset_residency_reentry(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.baseline.feature_label,
                    build_label: test_args.baseline.build_label,
                    output_path: test_args.baseline.output,
                    marker_timeout_secs: test_args.baseline.marker_timeout_secs,
                    hold_samples: 0,
                },
                test_args.cycles,
            )
        }
        TestSubcommand::AssetResidencyStress(test_args) => {
            workflows::asset_residency::run_asset_residency_stress(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.baseline.feature_label,
                    build_label: test_args.baseline.build_label,
                    output_path: test_args.baseline.output,
                    marker_timeout_secs: test_args.baseline.marker_timeout_secs,
                    hold_samples: test_args.hold_samples,
                },
            )
        }
        TestSubcommand::StackHeadroomWatch(test_args) => {
            workflows::asset_residency::run_stack_headroom_watch(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.feature_label,
                    build_label: test_args.build_label,
                    output_path: test_args.output,
                    marker_timeout_secs: test_args.marker_timeout_secs,
                    hold_samples: 7,
                },
            )
        }
        TestSubcommand::StackHeadroomWatchUploadOff(test_args) => {
            workflows::asset_residency::run_stack_headroom_watch_upload_off(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.feature_label,
                    build_label: test_args.build_label,
                    output_path: test_args.output,
                    marker_timeout_secs: test_args.marker_timeout_secs,
                    hold_samples: 7,
                },
            )
        }
        TestSubcommand::StackHeadroomWatchUploadOn(test_args) => {
            workflows::asset_residency::run_stack_headroom_watch_upload_on(
                logger,
                workflows::asset_residency::AssetResidencyBaselineOptions {
                    feature_label: test_args.feature_label,
                    build_label: test_args.build_label,
                    output_path: test_args.output,
                    marker_timeout_secs: test_args.marker_timeout_secs,
                    hold_samples: 7,
                },
            )
        }
        TestSubcommand::ObservationFixture(test_args) => {
            workflows::observation_fixture::run_observation_fixture(
                logger,
                ObservationFixtureOptions {
                    provider: test_args.provider,
                    period_ms: test_args.period_ms,
                    panel_cycle: test_args.panel_cycle,
                    cancel_after_samples: test_args.cancel_after_samples,
                    request_id: test_args.request_id,
                    validity_ms: test_args.validity_ms,
                    timeout_ms: test_args.timeout_ms,
                    observe_ms: test_args.observe_ms,
                    output_path: test_args.output,
                },
            )
        }
        TestSubcommand::SdUploadRecovery(options) => workflows::storage::upload_probe::run(
            logger,
            options,
            workflows::storage::upload_probe::Probe::SdRecovery,
        ),
        TestSubcommand::ObservationUpload(options) => workflows::storage::upload_probe::run(
            logger,
            options,
            workflows::storage::upload_probe::Probe::Observations,
        ),
    }
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli) {
        eprintln!("error: {err:?}");
        std::process::exit(1);
    }
}
