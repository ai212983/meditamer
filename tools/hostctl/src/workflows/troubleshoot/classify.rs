pub(super) fn classify_failure(stage: &str, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();

    if lower.contains("failed to open serial")
        || lower.contains("resource busy")
        || lower.contains("permission denied")
        || lower.contains("autodetection was not conclusive")
        || lower.contains("no such file or directory")
    {
        return "uart_transport".to_string();
    }

    if lower.contains("could not compile")
        || lower.contains("linker")
        || lower.contains("failed to run custom build")
        || lower.contains("cargo build")
    {
        return "build".to_string();
    }

    if lower.contains("flash timed out")
        || lower.contains("failed to connect")
        || lower.contains("invalid head")
        || lower.contains("espflash")
        || stage == "flash"
    {
        return "flash".to_string();
    }

    if runtime_subclass(detail).is_some() {
        return "runtime".to_string();
    }

    if lower.contains("dhcp_no_ipv4_stall")
        || lower.contains("dhcp/no-ipv4 stall")
        || lower.contains("connected-without-ipv4")
    {
        return "dhcp_no_ipv4_stall".to_string();
    }

    if lower.contains("missing pong")
        || lower.contains("missing state")
        || lower.contains("missing psram")
        || lower.contains("state err")
        || stage == "probe"
    {
        return "uart_protocol".to_string();
    }

    if lower.contains("missing:") || stage == "soak" {
        return "boot".to_string();
    }

    "unknown".to_string()
}

pub(super) fn runtime_subclass(detail: &str) -> Option<&'static str> {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("boot_reset reason=") {
        return Some("runtime_unexpected_reboot");
    }
    if lower.contains("guru meditation") {
        return Some("runtime_panic_guru");
    }
    if lower.contains("stack overflow") || lower.contains("stack smashing") {
        return Some("runtime_panic_stack");
    }
    if lower.contains("assertion failed") {
        return Some("runtime_panic_assert");
    }
    if lower.contains("panic") || lower.contains("backtrace") || lower.contains("abort") {
        return Some("runtime_panic_other");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{classify_failure, runtime_subclass};

    #[test]
    fn classify_transport() {
        assert_eq!(
            classify_failure("probe", "failed to open serial port /dev/cu.usbserial"),
            "uart_transport"
        );
    }

    #[test]
    fn classify_runtime() {
        assert_eq!(
            classify_failure("probe", "Guru Meditation Error: Core 0 panic'ed"),
            "runtime"
        );
        assert_eq!(
            runtime_subclass("Guru Meditation Error: Core 0 panic'ed"),
            Some("runtime_panic_guru")
        );
    }

    #[test]
    fn classify_flash_stage_defaults_to_flash() {
        assert_eq!(classify_failure("flash", "non-zero exit"), "flash");
    }

    #[test]
    fn classify_dhcp_no_ipv4_stall() {
        assert_eq!(
            classify_failure(
                "probe",
                "dhcp_no_ipv4_stall: connected-without-ipv4 observed 77 samples"
            ),
            "dhcp_no_ipv4_stall"
        );
    }

    #[test]
    fn classify_unexpected_reboot_as_runtime() {
        assert_eq!(
            classify_failure("probe", "BOOT_RESET reason=Software code=3"),
            "runtime"
        );
        assert_eq!(
            runtime_subclass("BOOT_RESET reason=Software code=3"),
            Some("runtime_unexpected_reboot")
        );
    }
}
