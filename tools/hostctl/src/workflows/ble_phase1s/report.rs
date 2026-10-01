use super::*;

impl BlePhase1sRuntime<'_> {
    pub(super) fn assert_stack_floors(&mut self) -> Result<()> {
        let main = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let violations = self.final_gate_violations(free_drift, block_drift)?;
        if !violations.is_empty() {
            bail!("Phase 1S final gate failed: {}", violations.join("; "));
        }
        self.logger.info(format!(
            "Phase 1S runtime checks passed: cycles={} cpu0_stack={} touch_stack={} uart_log_drops=0 report={}",
            self.samples.len(),
            main,
            touch,
            self.report_path.display()
        ));
        Ok(())
    }

    pub(super) fn finish_report(&self) -> Result<Phase1sReport> {
        validate_completed_samples(&self.samples, self.cycles)?;
        validate_completed_ble_samples(&self.ble_samples, self.cycles)?;
        validate_correlated_samples(&self.samples, &self.ble_samples)?;
        let cpu0_stack_headroom_min = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch_stack_headroom_min = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let uart_log_drops_baseline = self
            .uart_log_drops_baseline
            .ok_or_else(|| anyhow!("UART diagnostic drop baseline was not collected"))?;
        let uart_log_drops_final = self
            .uart_log_drops_final
            .ok_or_else(|| anyhow!("UART diagnostic final drop count was not collected"))?;
        let uart_log_drops_during_gate =
            validate_uart_drop_counter(uart_log_drops_baseline, uart_log_drops_final).ok();
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let violations = self.final_gate_violations(free_drift, block_drift)?;
        let gate_passed = violations.is_empty();
        let (console_drops_baseline, console_drops_final, console_drops_during_gate) =
            console_drop_aggregate(&self.console_drop_samples);
        Ok(Phase1sReport {
            schema_version: 4,
            gate_kind: "phase1s_wifi_ble_exclusive",
            board_id: self.board_id.clone(),
            build_id: self.identity.build_id.clone(),
            source_git_head: self.identity.git_head.clone(),
            source_dirty: self.identity.source_dirty,
            artifact_elf_sha256: self.identity.elf_sha256.clone(),
            artifact_app_sha256: self.identity.app_sha256.clone(),
            serial_port: self.port.clone(),
            network_ssid: self.ssid.clone(),
            completed_cycles: self.samples.len() as u32,
            completed_ble_cycles: self.ble_samples.len() as u32,
            first_ble_allocation_delta_bytes: self
                .ble_samples
                .first()
                .map(|sample| sample.allocation_delta),
            minimum_ble_active_internal_free_bytes: self
                .ble_samples
                .iter()
                .map(|sample| sample.active_free)
                .min(),
            minimum_serving_internal_free_bytes: self.serving_internal_free_min,
            minimum_serving_internal_alloc_charge_bytes: self.serving_internal_min_alloc_charge,
            minimum_serving_internal_alloc_internal_required: self
                .serving_internal_min_alloc_internal_required,
            minimum_serving_internal_alloc_wifi_rx_matched: self
                .serving_internal_min_alloc_wifi_rx_matched,
            minimum_serving_internal_alloc_correlation_stable: self
                .serving_internal_min_alloc_correlation_stable,
            minimum_serving_internal_alloc_released: self.serving_internal_min_alloc_released,
            post_warmup_free_drift: free_drift,
            post_warmup_largest_block_drift: block_drift,
            cpu0_stack_headroom_min: Some(cpu0_stack_headroom_min),
            touch_stack_headroom_min: Some(touch_stack_headroom_min),
            uart_log_drops_baseline: Some(uart_log_drops_baseline),
            uart_log_drops_final: Some(uart_log_drops_final),
            uart_log_drops_during_gate,
            telemetry_profile: self.telemetry_profile.as_str(),
            telemetry_original: self.telemetry_original,
            telemetry_applied: self.telemetry_applied,
            telemetry_restored: self.telemetry_restored,
            telemetry_restore_error: self.telemetry_restore_error.clone(),
            console_drop_samples: self.console_drop_samples.clone(),
            console_drops_baseline,
            console_drops_final,
            console_drops_during_gate,
            failure_stage: None,
            failure_reason: None,
            ownership_known: Some(true),
            pending_off: None,
            pending_ble_status: None,
            gate_passed,
            violations,
            off_samples: self.samples.clone(),
            ble_samples: self.ble_samples.clone(),
        })
    }

    pub(super) fn finish_failure_report(&self) -> Phase1sReport {
        let (free_drift, block_drift) = self.post_warmup_drifts();
        let uart_log_drops_during_gate = self
            .uart_log_drops_baseline
            .zip(self.uart_log_drops_final)
            .and_then(|(baseline, final_count)| {
                validate_uart_drop_counter(baseline, final_count).ok()
            });
        let reason = self
            .failure_reason
            .clone()
            .unwrap_or_else(|| "BLE lifecycle failed without a classified reason".to_owned());
        let (console_drops_baseline, console_drops_final, console_drops_during_gate) =
            console_drop_aggregate(&self.console_drop_samples);
        let mut violations = vec![reason.clone()];
        violations.extend(console_drop_violations(&self.console_drop_samples));
        Phase1sReport {
            schema_version: 4,
            gate_kind: "phase1s_wifi_ble_exclusive",
            board_id: self.board_id.clone(),
            build_id: self.identity.build_id.clone(),
            source_git_head: self.identity.git_head.clone(),
            source_dirty: self.identity.source_dirty,
            artifact_elf_sha256: self.identity.elf_sha256.clone(),
            artifact_app_sha256: self.identity.app_sha256.clone(),
            serial_port: self.port.clone(),
            network_ssid: self.ssid.clone(),
            completed_cycles: self.samples.len() as u32,
            completed_ble_cycles: self.ble_samples.len() as u32,
            first_ble_allocation_delta_bytes: self
                .ble_samples
                .first()
                .map(|sample| sample.allocation_delta),
            minimum_ble_active_internal_free_bytes: self
                .ble_samples
                .iter()
                .map(|sample| sample.active_free)
                .min(),
            minimum_serving_internal_free_bytes: self.serving_internal_free_min,
            minimum_serving_internal_alloc_charge_bytes: self.serving_internal_min_alloc_charge,
            minimum_serving_internal_alloc_internal_required: self
                .serving_internal_min_alloc_internal_required,
            minimum_serving_internal_alloc_wifi_rx_matched: self
                .serving_internal_min_alloc_wifi_rx_matched,
            minimum_serving_internal_alloc_correlation_stable: self
                .serving_internal_min_alloc_correlation_stable,
            minimum_serving_internal_alloc_released: self.serving_internal_min_alloc_released,
            post_warmup_free_drift: free_drift,
            post_warmup_largest_block_drift: block_drift,
            cpu0_stack_headroom_min: self.cpu0_stack_headroom_min,
            touch_stack_headroom_min: self.touch_stack_headroom_min,
            uart_log_drops_baseline: self.uart_log_drops_baseline,
            uart_log_drops_final: self.uart_log_drops_final,
            uart_log_drops_during_gate,
            telemetry_profile: self.telemetry_profile.as_str(),
            telemetry_original: self.telemetry_original,
            telemetry_applied: self.telemetry_applied,
            telemetry_restored: self.telemetry_restored,
            telemetry_restore_error: self.telemetry_restore_error.clone(),
            console_drop_samples: self.console_drop_samples.clone(),
            console_drops_baseline,
            console_drops_final,
            console_drops_during_gate,
            failure_stage: self.failure_stage.clone(),
            failure_reason: Some(reason.clone()),
            ownership_known: self.ownership_known,
            pending_off: self
                .pending_cycle
                .as_ref()
                .map(|pending| pending.off.clone()),
            pending_ble_status: self
                .pending_cycle
                .as_ref()
                .and_then(|pending| pending.ble_status.clone()),
            gate_passed: false,
            violations,
            off_samples: self.samples.clone(),
            ble_samples: self.ble_samples.clone(),
        }
    }

    fn post_warmup_drifts(&self) -> (u32, u32) {
        let warm = self.samples.iter().skip(1);
        (
            drift(warm.clone().map(|sample| sample.internal_free)),
            drift(warm.map(|sample| sample.largest_block)),
        )
    }

    fn final_gate_violations(&self, free_drift: u32, block_drift: u32) -> Result<Vec<String>> {
        let main = self
            .cpu0_stack_headroom_min
            .ok_or_else(|| anyhow!("CPU0 stack floor was not collected"))?;
        let touch = self
            .touch_stack_headroom_min
            .ok_or_else(|| anyhow!("touch-core stack floor was not collected"))?;
        let baseline = self
            .uart_log_drops_baseline
            .ok_or_else(|| anyhow!("UART diagnostic drop baseline was not collected"))?;
        let final_count = self
            .uart_log_drops_final
            .ok_or_else(|| anyhow!("UART diagnostic final drop count was not collected"))?;
        let mut violations = Vec::new();
        if let Err(error) = validate_stack_floors(main, touch) {
            violations.push(error.to_string());
        }
        match self.serving_internal_free_min {
            Some(minimum) if minimum >= REQUIRED_BLE_ACTIVE_FREE => {}
            Some(minimum) => violations.push(format!(
                "serving internal-free floor failed: minimum={} required={}",
                minimum, REQUIRED_BLE_ACTIVE_FREE
            )),
            None => violations.push("serving internal-free floor was not collected".to_owned()),
        }
        match validate_uart_drop_counter(baseline, final_count) {
            Ok(0) => {}
            Ok(during_gate) => violations.push(format!(
                "UART diagnostic drop gate failed: during_gate={during_gate}"
            )),
            Err(error) => violations.push(error.to_string()),
        }
        if free_drift > MAX_POST_WARMUP_DRIFT || block_drift > MAX_POST_WARMUP_DRIFT {
            violations.push(format!(
                "post-warm-up drift exceeded {} bytes: free={} largest_block={}",
                MAX_POST_WARMUP_DRIFT, free_drift, block_drift
            ));
        }
        let ble_active_drift = drift(
            self.ble_samples
                .iter()
                .skip(1)
                .map(|sample| sample.active_free),
        );
        let ble_after_drift = drift(
            self.ble_samples
                .iter()
                .skip(1)
                .map(|sample| sample.after_free),
        );
        if ble_active_drift > MAX_POST_WARMUP_DRIFT || ble_after_drift > MAX_POST_WARMUP_DRIFT {
            violations.push(format!(
                "BLE post-warm-up drift exceeded {} bytes: active={} after={}",
                MAX_POST_WARMUP_DRIFT, ble_active_drift, ble_after_drift
            ));
        }
        violations.extend(console_drop_violations(&self.console_drop_samples));
        match console_drop_aggregate(&self.console_drop_samples).2 {
            Some(0) => {}
            Some(drops) => violations.push(format!(
                "console stage drop gate failed: during_gate={drops}"
            )),
            None => violations.push("console stage drop interval was not collected".to_owned()),
        }
        Ok(violations)
    }

    pub(super) fn note_telemetry_restore_failure(&mut self, error: &str) {
        let detail = format!("telemetry restore failed: {error}");
        self.telemetry_restored = Some(false);
        self.telemetry_restore_error = Some(detail.clone());
        match self.failure_reason.take() {
            Some(reason) => self.failure_reason = Some(format!("{reason}; {detail}")),
            None => {
                self.failure_stage = Some("telemetry_restore".to_owned());
                self.failure_reason = Some(detail);
            }
        }
    }

    pub(super) fn refresh_persisted_report(&self) -> Result<()> {
        // A failed telemetry restore owns the persisted verdict even when the
        // measurement gate itself completed.
        let report = if self.failure_reason.is_some() || self.telemetry_restore_error.is_some() {
            serde_json::to_vec_pretty(&self.finish_failure_report())?
        } else {
            match self.finish_report() {
                Ok(report) => serde_json::to_vec_pretty(&report)?,
                Err(_) => serde_json::to_vec_pretty(&self.finish_failure_report())?,
            }
        };
        fs::write(&self.report_path, report)?;
        Ok(())
    }
}
