use crate::serial_console::SerialConsole;

pub(super) fn recent_uart_lines(console: &SerialConsole, max_lines: usize) -> String {
    let lines = console.read_recent_lines(0);
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

pub(super) fn format_command_output(output: &std::process::Output) -> String {
    let mut joined = String::new();

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !stdout.is_empty() {
        joined.push_str("stdout:\n");
        joined.push_str(&tail_lines(&stdout, 60));
    }
    if !stderr.is_empty() {
        if !joined.is_empty() {
            joined.push('\n');
        }
        joined.push_str("stderr:\n");
        joined.push_str(&tail_lines(&stderr, 60));
    }

    if joined.is_empty() {
        "(no command output captured)".to_string()
    } else {
        joined
    }
}

fn tail_lines(input: &str, max_lines: usize) -> String {
    let lines = input.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}
