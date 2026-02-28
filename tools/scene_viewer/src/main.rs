mod bundle;
mod cli;
mod io;
mod render;

use std::env;

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        cli::print_help();
        return Ok(());
    };

    match cmd.as_str() {
        "render" => render::run_render(args),
        "inspect" => bundle::run_inspect(args),
        "help" | "--help" | "-h" => {
            cli::print_help();
            Ok(())
        }
        _ => Err(format!("unknown command '{cmd}'")),
    }
}
