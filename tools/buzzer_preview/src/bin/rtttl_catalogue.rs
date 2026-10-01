//! `rtttl_catalogue`: compile a notification RTTTL catalogue to one Rust module.
//!
//! Usage:
//!
//! ```text
//! rtttl_catalogue --in assets/sounds/notifications.rtttl --out <module.rs> [--transpose N]
//! ```
//!
//! The input holds one RTTTL score per line; blank lines and `#` comment
//! lines are skipped. `--transpose N` applies one signed whole-score
//! semitone shift to every note of every score (`0`, the default,
//! reproduces the unshifted catalogue). Every entry is validated before
//! anything is written: a failed run prints the error, leaves an existing
//! output file untouched, and exits nonzero. Output is deterministic: the
//! same catalogue and transpose always render byte-identical Rust.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("usage: rtttl_catalogue --in <catalogue.rtttl> --out <module.rs> [--transpose N]");
        return ExitCode::SUCCESS;
    }
    let cli = match buzzer_preview::parse_catalogue_cli(&args) {
        Ok(cli) => cli,
        Err(error) => {
            eprintln!("rtttl_catalogue: {error}");
            return ExitCode::FAILURE;
        }
    };
    // `regenerate` validates the whole catalogue before touching `--out`,
    // so a failed run preserves the previous working module.
    match buzzer_preview::catalogue::regenerate(&cli.input, &cli.output, cli.transpose) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rtttl_catalogue: {error}");
            ExitCode::FAILURE
        }
    }
}
