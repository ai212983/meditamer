//! Read-only serial attachment that preserves the port's modem-control lines.

use crate::{env_utils, serial_console::SerialConsole, workflows::wifi::common::acquire_port_lock};
use anyhow::Result;
use std::io::{self, Write};

pub fn run() -> Result<()> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115_200)?;
    let _lock = acquire_port_lock(&port)?;
    let mut console = SerialConsole::open_passive(&port, baud, None)?;
    let stdout = io::stdout();
    let mut output = stdout.lock();

    loop {
        let cursor = console.mark();
        console.poll_once()?;
        for line in console.read_recent_lines(cursor) {
            writeln!(output, "{line}")?;
        }
        output.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    #[test]
    fn passive_monitor_is_a_bare_observation_command() {
        let cli = crate::Cli::try_parse_from(["hostctl", "monitor"]).expect("parse monitor");
        assert!(matches!(cli.command, crate::Commands::Monitor));
        assert!(crate::Cli::try_parse_from(["hostctl", "monitor", "--command", "PING"]).is_err());
    }
}
