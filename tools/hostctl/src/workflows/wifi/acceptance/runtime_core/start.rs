impl WifiAcceptanceRuntime<'_> {
    fn handle_prepare_payload(&mut self) -> Result<()> {
        ensure_parent_dir(&self.payload_path)?;
        let mut data = vec![0u8; 524_288];
        for (i, slot) in data.iter_mut().enumerate() {
            *slot = ((i * 17 + 31) & 0xFF) as u8;
        }
        fs::write(&self.payload_path, data)?;
        Ok(())
    }

    fn handle_start_run(&mut self, context: &mut Value) -> Result<()> {
        self.ensure_operating_upload_mode()?;
        ctx_set_u32(context, "cycle", 1)?;
        ctx_set_u32(context, "cycles", self.cycles)?;
        ctx_set_u32(context, "operation_retries", self.operation_retries)?;
        self.mem_read_mark = self.console.mark();
        self.panic_monitoring_enabled = true;
        self.panic_first = None;
        self.req_read_body_reset_baseline = Some(self.query_req_read_body_reset()?);
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

    fn handle_init_upload_attempt(&self, context: &mut Value) -> Result<()> {
        ctx_set_u32(context, "upload_attempt", 1)?;
        ctx_set_bool(context, "upload_done", false)?;
        Ok(())
    }
}
