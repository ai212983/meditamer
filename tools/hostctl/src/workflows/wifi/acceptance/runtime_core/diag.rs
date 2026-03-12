impl WifiAcceptanceRuntime<'_> {
    pub(super) fn capture_mem_diag_lines(&mut self) -> Result<()> {
        self.console.poll_once()?;
        for line in self.console.read_recent_lines(self.mem_read_mark) {
            let line_index = self.mem_read_mark;
            self.mem_read_mark = self.mem_read_mark.saturating_add(1);
            self.mem_diag.record_line(&line);
            if self.panic_monitoring_enabled && self.panic_first.is_none() {
                if let Some(signal) = detect_panic_signal(&line, line_index) {
                    self.panic_first = Some(signal.clone());
                    return Err(anyhow!(self.panic_signal_detail(&signal)));
                }
            }
        }
        Ok(())
    }

    pub(super) fn panic_signal(&self) -> Option<&PanicSignal> {
        self.panic_first.as_ref()
    }

    fn panic_signal_detail(&self, signal: &PanicSignal) -> String {
        let excerpt_start = signal.marker_index.saturating_sub(3);
        let excerpt_source = self.console.read_recent_lines(excerpt_start);
        let excerpt = format_context_excerpt(extract_context_window(
            &excerpt_source,
            excerpt_start,
            signal.marker_index,
            3,
        ));
        match excerpt {
            Some(window) => format!(
                "panic_detected class={} line_index={} line={} context={window}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ),
            None => format!(
                "panic_detected class={} line_index={} line={}",
                signal.class.as_str(),
                signal.marker_index,
                signal.marker_line
            ),
        }
    }

    pub(super) fn log_mem_summary(&mut self, prefix: &str) {
        self.logger.info(format!(
            "{prefix} mem samples={} radio_samples={} upload_samples={} nomem_stage_samples={} min_internal_free={} min_external_free={} min_total_free={} min_internal_low_water={}",
            self.mem_diag.samples,
            self.mem_diag.radio_samples,
            self.mem_diag.upload_samples,
            self.mem_diag.nomem_stage_samples,
            fmt_min(&self.mem_diag.min_internal_free),
            fmt_min(&self.mem_diag.min_external_free),
            fmt_min(&self.mem_diag.min_free),
            fmt_min(&self.mem_diag.min_internal_low_water),
        ));
    }
}

fn format_context_excerpt(window: Vec<(usize, String)>) -> Option<String> {
    if window.is_empty() {
        return None;
    }
    Some(
        window
            .into_iter()
            .map(|(idx, line)| format!("{idx}:{line}"))
            .collect::<Vec<_>>()
            .join(" | "),
    )
}
