mod env_utils;
mod logging;
mod port_detect;
mod scenarios;
mod serial_console;
mod workflows_runtime_modes;
mod workflows_sdcard;
#[cfg(test)]
mod workflows_sdcard_tests;
mod workflows_serial;
mod workflows_troubleshoot;
mod workflows_upload;
mod workflows_wifi_acceptance;
mod workflows_wifi_discovery;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use logging::Logger;

use workflows_runtime_modes::RuntimeModesSmokeOptions;
use workflows_sdcard::{SdcardHwOptions, SdcardSuite};
use workflows_serial::{RepaintOptions, TimeSetOptions, TouchWizardDumpOptions};
use workflows_troubleshoot::TroubleshootOptions;
use workflows_upload::UploadOptions;
use workflows_wifi_acceptance::WifiAcceptanceOptions;
use workflows_wifi_discovery::WifiDiscoveryDebugOptions;

#[derive(Debug, Parser)]
#[command(name = "hostctl")]
#[command(about = "Meditamer host instrumentation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Timeset(TimeSetArgs),
    Repaint(RepaintArgs),
    MarbleMetrics,
    TouchWizardDump(TouchWizardDumpArgs),
    Upload(UploadArgs),
    Test(TestArgs),
}

#[derive(Debug, Args)]
struct TimeSetArgs {
    #[arg(long)]
    epoch: Option<u64>,
    #[arg(long = "tz-offset-minutes")]
    tz_offset_minutes: Option<i32>,
}

#[derive(Debug, Args)]
struct RepaintArgs {
    #[arg(long)]
    command: Option<String>,
}

#[derive(Debug, Args)]
struct TouchWizardDumpArgs {
    #[arg(long)]
    output: Option<PathBuf>,
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
    WifiAcceptance(WifiAcceptanceArgs),
    WifiDiscoveryDebug(WifiDiscoveryDebugArgs),
    RuntimeModesSmoke(RuntimeModesArgs),
    SdcardHw(SdcardArgs),
    SdcardBurstRegression(SdcardBurstArgs),
    Troubleshoot(TroubleshootArgs),
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

fn parse_suite(raw: &str) -> Result<SdcardSuite> {
    match raw {
        "all" => Ok(SdcardSuite::All),
        "baseline" => Ok(SdcardSuite::Baseline),
        "burst" => Ok(SdcardSuite::Burst),
        "failures" => Ok(SdcardSuite::Failures),
        _ => Err(anyhow::anyhow!(
            "Invalid suite `{raw}` (use all|baseline|burst|failures)"
        )),
    }
}

fn run(cli: Cli) -> Result<()> {
    let mut logger = Logger::from_env()?;

    match cli.command {
        Commands::Timeset(args) => workflows_serial::run_timeset(
            &mut logger,
            TimeSetOptions {
                epoch: args.epoch,
                tz_offset_minutes: args.tz_offset_minutes,
            },
        ),
        Commands::Repaint(args) => workflows_serial::run_repaint(
            &mut logger,
            RepaintOptions {
                command: args.command,
            },
        ),
        Commands::MarbleMetrics => workflows_serial::run_marble_metrics(&mut logger),
        Commands::TouchWizardDump(args) => workflows_serial::run_touch_wizard_dump(
            &mut logger,
            TouchWizardDumpOptions {
                output_path: args.output,
            },
        ),
        Commands::Upload(args) => workflows_upload::run_upload(
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
            TestSubcommand::WifiAcceptance(test_args) => {
                workflows_wifi_acceptance::run_wifi_acceptance(
                    &mut logger,
                    WifiAcceptanceOptions {
                        output_path: test_args.output_path,
                    },
                )
            }
            TestSubcommand::WifiDiscoveryDebug(test_args) => {
                workflows_wifi_discovery::run_wifi_discovery_debug(
                    &mut logger,
                    WifiDiscoveryDebugOptions {
                        output_path: test_args.output_path,
                    },
                )
            }
            TestSubcommand::RuntimeModesSmoke(test_args) => {
                workflows_runtime_modes::run_runtime_modes_smoke(
                    &mut logger,
                    RuntimeModesSmokeOptions {
                        output_path: test_args.output_path,
                    },
                )
            }
            TestSubcommand::SdcardHw(test_args) => workflows_sdcard::run_sdcard_hw(
                &mut logger,
                SdcardHwOptions {
                    build_mode: test_args.build_mode,
                    output_path: test_args.output,
                    suite: parse_suite(&test_args.suite)?,
                },
            ),
            TestSubcommand::SdcardBurstRegression(test_args) => {
                workflows_sdcard::run_sdcard_burst_regression(
                    &mut logger,
                    test_args.build_mode,
                    test_args.output,
                )
            }
            TestSubcommand::Troubleshoot(test_args) => workflows_troubleshoot::run_troubleshoot(
                &mut logger,
                TroubleshootOptions {
                    build_mode: test_args.build_mode,
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
