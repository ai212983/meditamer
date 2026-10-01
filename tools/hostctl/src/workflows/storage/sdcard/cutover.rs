use std::time::Duration;

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::Value;

use super::runtime::SdcardScenarioRuntime;

impl SdcardScenarioRuntime<'_> {
    pub(super) fn invoke_cutover_summary(&mut self, args: &Value) -> Result<()> {
        let label = args
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("SD_CUTOVER_SUMMARY");
        let touch_re = Regex::new(r"^METRICS TOUCH_SCHED ")?;
        let memory_re = Regex::new(r"^PSRAM ")?;
        if self
            .console
            .command_wait_regex("METRICS", &touch_re, Duration::from_secs(5))?
            .is_none()
        {
            return Err(anyhow!("cutover summary: missing touch scheduling metrics"));
        }
        if self
            .console
            .command_wait_regex("PSRAM", &memory_re, Duration::from_secs(5))?
            .is_none()
        {
            return Err(anyhow!("cutover summary: missing allocator metrics"));
        }

        let lines = self.console.read_recent_lines(self.scenario_mark);
        let stack_re = Regex::new(r"^stack_diag: .*headroom=([0-9]+)")?;
        let touch_stack_re = Regex::new(r"^touch_core_stack_diag: .*headroom=([0-9]+)")?;
        if contains_unexpected_runtime_fault(&lines)? {
            return Err(anyhow!(
                "cutover summary: panic, reset, or SD timeout signature detected"
            ));
        }
        let min_stack = lines
            .iter()
            .filter_map(|line| capture_u32(&stack_re, line))
            .min()
            .ok_or_else(|| anyhow!("cutover summary: no stack_diag samples"))?;
        let min_touch_stack = lines
            .iter()
            .filter_map(|line| capture_u32(&touch_stack_re, line))
            .min()
            .ok_or_else(|| anyhow!("cutover summary: no touch-core stack samples"))?;
        let touch = lines
            .iter()
            .rev()
            .find(|line| touch_re.is_match(line))
            .ok_or_else(|| anyhow!("cutover summary: no touch metrics line"))?;
        let memory = lines
            .iter()
            .rev()
            .find(|line| memory_re.is_match(line))
            .ok_or_else(|| anyhow!("cutover summary: no allocator line"))?;
        let active_gap = token_u32(touch, "active_gap_max_ms")?;
        let min_internal = token_u32(memory, "min_internal_free_bytes")?;

        self.logger.info(format!(
            "{label} min_stack_headroom={} min_touch_core_stack_headroom={} min_internal_free={} active_gap_max_ms={}",
            min_stack, min_touch_stack, min_internal, active_gap
        ));
        if min_stack < 8 * 1024 {
            return Err(anyhow!(
                "cutover summary: stack headroom {min_stack} < 8192"
            ));
        }
        if min_internal < 16 * 1024 {
            return Err(anyhow!(
                "cutover summary: internal free {min_internal} < 16384"
            ));
        }
        if min_touch_stack < 1024 {
            return Err(anyhow!(
                "cutover summary: touch-core stack headroom {min_touch_stack} < 1024"
            ));
        }
        if active_gap > 16 {
            return Err(anyhow!(
                "cutover summary: active touch gap {active_gap} > 16"
            ));
        }
        Ok(())
    }
}

fn capture_u32(regex: &Regex, line: &str) -> Option<u32> {
    regex.captures(line)?.get(1)?.as_str().parse().ok()
}

fn token_u32(line: &str, key: &str) -> Result<u32> {
    line.split_ascii_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| anyhow!("missing numeric token {key} in line: {line}"))
}

fn contains_unexpected_runtime_fault(lines: &[String]) -> Result<bool> {
    let panic_re = Regex::new(r"(?i)(panic|guru meditation|stack guard)")?;
    let reset_re = Regex::new(r"(?i)(rst:0x|BOOT_RESET reason=)")?;
    let sd_timeout_re = Regex::new(
        r"(?i)^(sdtask: .*timeout|sdprobe(?:\[[^]]+\])?: .*timeout|sdrw\[[^]]+\]: .*timeout|sd_upload: .*timeout)",
    )?;
    let ready_re = Regex::new(r"^RUNTIME_READY\b")?;
    let mut runtime_ready = false;

    for line in lines {
        if panic_re.is_match(line) {
            return Ok(true);
        }
        if sd_timeout_re.is_match(line) {
            return Ok(true);
        }
        if runtime_ready && reset_re.is_match(line) {
            return Ok(true);
        }
        if ready_re.is_match(line) {
            runtime_ready = true;
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{capture_u32, contains_unexpected_runtime_fault};
    use regex::Regex;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|line| (*line).to_string()).collect()
    }

    #[test]
    fn main_stack_pattern_does_not_capture_touch_core_stack() {
        let stack_re = Regex::new(r"^stack_diag: .*headroom=([0-9]+)").unwrap();
        assert_eq!(
            capture_u32(&stack_re, "stack_diag: tag=minimum headroom=25976"),
            Some(25_976)
        );
        assert_eq!(
            capture_u32(
                &stack_re,
                "touch_core_stack_diag: tag=minimum headroom=3220"
            ),
            None
        );
    }

    #[test]
    fn startup_reset_before_runtime_ready_is_expected() {
        let input = lines(&[
            "rst:0x1 (POWERON_RESET),boot:0x13 (SPI_FAST_FLASH_BOOT)",
            "BOOT_RESET reason=Some(ChipPowerOn) code=1",
            "RUNTIME_READY app_state=ready display=ready",
        ]);
        assert!(!contains_unexpected_runtime_fault(&input).unwrap());
    }

    #[test]
    fn reset_after_runtime_ready_is_rejected() {
        let input = lines(&[
            "RUNTIME_READY app_state=ready display=ready",
            "rst:0xc (SW_CPU_RESET),boot:0x13 (SPI_FAST_FLASH_BOOT)",
        ]);
        assert!(contains_unexpected_runtime_fault(&input).unwrap());
    }

    #[test]
    fn panic_is_rejected_even_before_runtime_ready() {
        let input = lines(&["Guru Meditation Error: Core 0 panic'ed"]);
        assert!(contains_unexpected_runtime_fault(&input).unwrap());
    }

    #[test]
    fn sd_power_response_timeout_is_rejected() {
        let input = lines(&[
            "RUNTIME_READY app_state=ready display=ready",
            "sdtask: power_resp_timeout action=on timeout_ms=1500 attempt=1/4",
        ]);
        assert!(contains_unexpected_runtime_fault(&input).unwrap());
    }

    #[test]
    fn timeout_metric_names_are_not_runtime_faults() {
        let input = lines(&[
            "RUNTIME_READY app_state=ready display=ready",
            "METRICS UPLOAD sd_timeouts=0 sess_timeout_abort=0",
        ]);
        assert!(!contains_unexpected_runtime_fault(&input).unwrap());
    }
}
