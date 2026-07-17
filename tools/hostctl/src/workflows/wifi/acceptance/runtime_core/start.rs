impl WifiAcceptanceRuntime<'_> {
    fn handle_wait_runtime_ready(&mut self) -> Result<()> {
        let ready_marker = Regex::new(r"^RUNTIME_READY app_state=ready display=ready$")?;
        let ready_status = Regex::new(r"^SCHEDPROFILE .*runtime_ready=on$")?;
        let timeout_ms = env_utils::parse_env_u32("HOSTCTL_NET_RUNTIME_READY_TIMEOUT_MS", 45_000)?;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.into());
        while Instant::now() < deadline {
            if self
                .console
                .find_first_regex_since(0, &ready_marker)
                .is_some()
                || self
                    .console
                    .command_wait_regex(
                        "SCHEDPROFILE",
                        &ready_status,
                        Duration::from_millis(750),
                    )?
                    .is_some()
            {
                self.logger.info("runtime readiness gate: pass");
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(anyhow!(
            "runtime readiness not observed within {} ms",
            timeout_ms
        ))
    }

    fn handle_boot_discovery_gate(&mut self) -> Result<()> {
        if !env_utils::parse_env_bool01("HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE", true)? {
            return Ok(());
        }
        let cfg = BootDiscoveryGateConfig {
            max_boot_uptime_ms: env_utils::parse_env_u32(
                "HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS",
                30_000,
            )?,
            timeout_ms: env_utils::parse_env_u32(
                "HOSTCTL_NET_BOOT_DISCOVERY_TIMEOUT_MS",
                180_000,
            )?,
            settle_ms: env_utils::parse_env_u32(
                "HOSTCTL_NET_BOOT_DISCOVERY_SETTLE_MS",
                6_000,
            )?,
            allow_ready_only_fallback: env_utils::parse_env_bool01(
                "HOSTCTL_NET_BOOT_DISCOVERY_READY_ONLY_FALLBACK",
                false,
            )?,
        };
        run_boot_discovery_gate(
            self.logger,
            &mut self.console,
            &self.ssid,
            &self.password,
            self.policy,
            cfg,
        )
    }

    fn handle_prepare_payload(&mut self) -> Result<()> {
        ensure_parent_dir(&self.payload_path)?;
        let mut data = vec![0u8; 524_288];
        for (i, slot) in data.iter_mut().enumerate() {
            *slot = ((i * 17 + 31) & 0xFF) as u8;
        }
        fs::write(&self.payload_path, data)?;
        Ok(())
    }

    fn build_start_run_result(&self) -> Value {
        serde_json::json!({
            "cycle": 1,
            "cycles": self.cycles,
            "operation_retries": self.operation_retries
        })
    }

    fn handle_start_run(&mut self) -> Result<()> {
        self.ensure_operating_upload_mode()?;
        self.mem_read_mark = self.console.mark();
        self.panic_monitoring_enabled = true;
        self.panic_first = None;
        self.req_read_body_reset_baseline = Some(self.query_req_read_body_reset()?);
        Ok(())
    }

    fn handle_prepare_measurement(&mut self) -> Result<()> {
        let quiet = Regex::new(r"^TELEMSET OK mask=0x00 ")?;
        self.console
            .command_wait_regex("TELEMSET ALL OFF", &quiet, Duration::from_secs(3))?
            .ok_or_else(|| anyhow!("telemetry quiet-mode acknowledgement not observed"))?;

        let reset = Regex::new(r"^TOUCHSCHEDRESET OK$")?;
        self.console
            .command_wait_regex("TOUCHSCHEDRESET", &reset, Duration::from_secs(3))?
            .ok_or_else(|| anyhow!("touch scheduler reset acknowledgement not observed"))?;
        self.logger
            .info("measurement window: verbose telemetry off; touch scheduler reset");
        Ok(())
    }

    fn handle_assert_runtime_health(&mut self) -> Result<()> {
        let touch_re = Regex::new(r"^METRICS TOUCH_SCHED ")?;
        let touch_line = self
            .console
            .command_wait_regex("METRICS", &touch_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing METRICS TOUCH_SCHED response"))?;
        let loop_gap = metric_u32(&touch_line, "loop_gap_max_ms")
            .ok_or_else(|| anyhow!("touch metrics missing loop_gap_max_ms: {touch_line}"))?;
        let active_gap = metric_u32(&touch_line, "active_gap_max_ms")
            .ok_or_else(|| anyhow!("touch metrics missing active_gap_max_ms: {touch_line}"))?;
        let loop_limit = env_utils::parse_env_u32("HOSTCTL_TOUCH_LOOP_GAP_MAX_MS", 8)?;
        let active_limit = env_utils::parse_env_u32("HOSTCTL_TOUCH_ACTIVE_GAP_MAX_MS", 16)?;
        if loop_gap > loop_limit || active_gap > active_limit {
            return Err(anyhow!(
                "touch scheduling gate failed: loop_gap_max_ms={} limit={} active_gap_max_ms={} limit={}",
                loop_gap,
                loop_limit,
                active_gap,
                active_limit
            ));
        }

        let main_stack_re = Regex::new(r"^stack_diag: tag=minimum headroom=")?;
        let main_stack_line = self
            .console
            .command_wait_regex("METRICS", &main_stack_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing main stack headroom response"))?;
        let main_stack_headroom = metric_u32(&main_stack_line, "headroom")
            .ok_or_else(|| anyhow!("main stack response missing headroom: {main_stack_line}"))?;
        let main_stack_floor =
            env_utils::parse_env_u32("HOSTCTL_MAIN_STACK_HEADROOM_MIN_BYTES", 8 * 1024)?;
        if main_stack_headroom < main_stack_floor {
            return Err(anyhow!(
                "main stack gate failed: headroom={} floor={}",
                main_stack_headroom,
                main_stack_floor
            ));
        }

        let touch_stack_re = Regex::new(r"^touch_core_stack_diag: tag=minimum headroom=")?;
        let touch_stack_line = self
            .console
            .command_wait_regex("METRICS", &touch_stack_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing touch-core stack headroom response"))?;
        let touch_stack_headroom = metric_u32(&touch_stack_line, "headroom")
            .ok_or_else(|| anyhow!("touch-core stack response missing headroom: {touch_stack_line}"))?;
        let touch_stack_floor =
            env_utils::parse_env_u32("HOSTCTL_TOUCH_CORE_STACK_HEADROOM_MIN_BYTES", 1024)?;
        if touch_stack_headroom < touch_stack_floor {
            return Err(anyhow!(
                "touch-core stack gate failed: headroom={} floor={}",
                touch_stack_headroom,
                touch_stack_floor
            ));
        }

        let memory_re = Regex::new(r"^PSRAM ")?;
        let memory_line = self
            .console
            .command_wait_regex("PSRAM", &memory_re, Duration::from_secs(5))?
            .ok_or_else(|| anyhow!("missing PSRAM allocator response"))?;
        let min_internal = metric_u32(&memory_line, "min_internal_free_bytes")
            .ok_or_else(|| anyhow!("PSRAM status missing min_internal_free_bytes: {memory_line}"))?;
        let memory_floor =
            env_utils::parse_env_u32("HOSTCTL_NET_MIN_INTERNAL_FREE_BYTES", 16 * 1024)?;
        if min_internal < memory_floor {
            return Err(anyhow!(
                "internal memory gate failed: min_internal_free_bytes={} floor={}",
                min_internal,
                memory_floor
            ));
        }

        self.logger.info(format!(
            "runtime_health_gate: loop_gap_max_ms={} active_gap_max_ms={} main_stack_headroom={} touch_core_stack_headroom={} min_internal_free_bytes={}",
            loop_gap,
            active_gap,
            main_stack_headroom,
            touch_stack_headroom,
            min_internal
        ));
        Ok(())
    }

    fn ensure_operating_upload_mode(&mut self) -> Result<()> {
        if !env_utils::parse_env_bool01("HOSTCTL_NET_ENSURE_OPERATING_MODE", true)? {
            return Ok(());
        }
        self.send_state_command_with_ack("STATE SET upload=on")?;
        self.send_state_command_with_ack("STATE DIAG kind=NONE targets=NONE")?;
        let state_re = Regex::new(r"^STATE phase=").expect("state regex");
        let mark = self.console.mark();
        self.console.send_line("STATE GET")?;
        if let Some(line) =
            self.console
                .wait_for_regex_since(mark, &state_re, Duration::from_secs(4))?
        {
            self.logger.info(format!("net_start state_probe: {line}"));
        }
        self.console.settle(200)?;
        Ok(())
    }

    fn send_state_command_with_ack(&mut self, command: &str) -> Result<()> {
        const ATTEMPTS: usize = 4;
        for attempt in 1..=ATTEMPTS {
            let mark = self.console.mark();
            self.console.send_line(command)?;
            let (status, line) =
                self.console
                    .wait_ack_since(mark, "STATE", Duration::from_secs(4))?;
            match status {
                AckStatus::Ok => return Ok(()),
                AckStatus::Busy | AckStatus::None => {
                    if attempt < ATTEMPTS {
                        thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                    return Err(anyhow!(
                        "state command did not ack after {} attempt(s): {}",
                        ATTEMPTS,
                        command
                    ));
                }
                AckStatus::Err => {
                    return Err(anyhow!(
                        "state command failed: {} ({})",
                        command,
                        line.unwrap_or_else(|| "STATE ERR".to_string())
                    ));
                }
            }
        }
        Err(anyhow!("unreachable state-ack retry path"))
    }

    fn build_init_upload_attempt_result(&self) -> Value {
        serde_json::json!({
            "upload_attempt": 1,
            "upload_done": false
        })
    }

    fn handle_init_upload_attempt(&self) -> Result<()> {
        Ok(())
    }
}

fn metric_u32(line: &str, key: &str) -> Option<u32> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse::<u32>().ok())
}
