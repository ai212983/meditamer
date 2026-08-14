#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanicClass {
    PanicStack,
    PanicAssert,
    PanicGuru,
    PanicWatchdog,
    PanicOther,
    UnexpectedReboot,
}

impl PanicClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PanicStack => "runtime_panic_stack",
            Self::PanicAssert => "runtime_panic_assert",
            Self::PanicGuru => "runtime_panic_guru",
            Self::PanicWatchdog => "runtime_panic_watchdog",
            Self::PanicOther => "runtime_panic_other",
            Self::UnexpectedReboot => "runtime_unexpected_reboot",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PanicSignal {
    pub class: PanicClass,
    pub marker_line: String,
    pub marker_index: usize,
}

pub fn detect_panic_signal(line: &str, line_index: usize) -> Option<PanicSignal> {
    let class = classify_panic_line(line)?;
    Some(PanicSignal {
        class,
        marker_line: line.to_string(),
        marker_index: line_index,
    })
}

pub fn extract_context_window(
    lines: &[String],
    start_index: usize,
    marker_index: usize,
    context_radius: usize,
) -> Vec<(usize, String)> {
    if lines.is_empty() {
        return Vec::new();
    }

    let first_index = start_index;
    let last_index = start_index + lines.len().saturating_sub(1);
    if marker_index < first_index || marker_index > last_index {
        return Vec::new();
    }

    let begin = marker_index.saturating_sub(context_radius).max(first_index);
    let end = marker_index.saturating_add(context_radius).min(last_index);
    let mut out = Vec::new();
    for idx in begin..=end {
        let offset = idx.saturating_sub(start_index);
        if let Some(line) = lines.get(offset) {
            out.push((idx, line.clone()));
        }
    }
    out
}

fn classify_panic_line(line: &str) -> Option<PanicClass> {
    let lower = line.to_ascii_lowercase();

    if lower.contains("boot_reset reason=") {
        return Some(PanicClass::UnexpectedReboot);
    }
    if lower.contains("guru meditation") {
        return Some(PanicClass::PanicGuru);
    }
    if lower.contains("task watchdog got triggered")
        || lower.contains("interrupt wdt timeout")
        || lower.contains("interrupt watchdog timeout")
    {
        return Some(PanicClass::PanicWatchdog);
    }
    if lower.contains("stack overflow") || lower.contains("stack smashing") {
        return Some(PanicClass::PanicStack);
    }
    if lower.contains("assertion failed") {
        return Some(PanicClass::PanicAssert);
    }
    if lower.contains("panic") || lower.contains("backtrace") {
        return Some(PanicClass::PanicOther);
    }
    // Keep abort-based panic detection, but avoid matching telemetry keys like
    // `sess_timeout_abort=<n>` in METRICS lines.
    if lower.contains("abort()") || lower.contains("abort was called") || lower.contains("aborted")
    {
        return Some(PanicClass::PanicOther);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{detect_panic_signal, extract_context_window, PanicClass};

    #[test]
    fn detects_guru_signature() {
        let signal =
            detect_panic_signal("Guru Meditation Error: Core 0 panic'ed", 17).expect("must detect");
        assert_eq!(signal.class, PanicClass::PanicGuru);
        assert_eq!(signal.marker_index, 17);
    }

    #[test]
    fn detects_stack_signature() {
        let signal = detect_panic_signal("fatal: stack overflow in task", 9).expect("must detect");
        assert_eq!(signal.class, PanicClass::PanicStack);
    }

    #[test]
    fn detects_task_and_interrupt_watchdogs() {
        for line in [
            "E task_wdt: Task watchdog got triggered. The following tasks did not reset",
            "Interrupt wdt timeout on CPU0",
        ] {
            let signal = detect_panic_signal(line, 11).expect("must detect watchdog");
            assert_eq!(signal.class, PanicClass::PanicWatchdog);
        }
    }

    #[test]
    fn detects_assert_signature() {
        let signal = detect_panic_signal("assertion failed: x < y", 9).expect("must detect");
        assert_eq!(signal.class, PanicClass::PanicAssert);
    }

    #[test]
    fn detects_unexpected_reboot_marker() {
        let signal =
            detect_panic_signal("BOOT_RESET reason=Software code=3", 5).expect("must detect");
        assert_eq!(signal.class, PanicClass::UnexpectedReboot);
    }

    #[test]
    fn ignores_non_panic_lines() {
        assert!(detect_panic_signal("NET_STATUS {\"state\":\"Ready\"}", 3).is_none());
        assert!(detect_panic_signal("METRICS UPLOAD sess_timeout_abort=0", 4).is_none());
    }

    #[test]
    fn detects_abort_signature_without_false_metric_match() {
        let signal =
            detect_panic_signal("abort() was called at PC 0x40000000", 7).expect("must detect");
        assert_eq!(signal.class, PanicClass::PanicOther);
    }

    #[test]
    fn extracts_context_window_around_marker() {
        let lines = vec![
            "a".to_string(),
            "b".to_string(),
            "panic marker".to_string(),
            "d".to_string(),
            "e".to_string(),
        ];
        let window = extract_context_window(&lines, 10, 12, 1);
        assert_eq!(
            window,
            vec![
                (11, "b".to_string()),
                (12, "panic marker".to_string()),
                (13, "d".to_string())
            ]
        );
    }
}
