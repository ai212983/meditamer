mod cli;
mod format;
mod inspect;
mod pipeline;

use std::env;

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        cli::print_help();
        return Ok(());
    };

    match cmd.as_str() {
        "build" => pipeline::run_build(args),
        "inspect" => pipeline::run_inspect(args),
        "-h" | "--help" | "help" => {
            cli::print_help();
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'")),
    }
}
