impl WifiAcceptanceRuntime<'_> {
    fn handle_net_apply_config(&mut self) -> Result<()> {
        if query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| is_ready(status, true))
        {
            self.logger
                .info("net_apply_config: skip NETCFG SET because network is already ready");
            return Ok(());
        }
        let payload = netcfg_set_payload(&self.ssid, &self.password, self.policy);
        wait_net_ack(&mut self.console, &format!("NETCFG SET {payload}"))
    }

    fn handle_net_start(&mut self) -> Result<()> {
        if self.should_skip_net_start_if_ready()? {
            return Ok(());
        }
        self.ensure_listener_on()?;
        if let Some(status) = query_net_status(&mut self.console)? {
            if self.should_return_from_status(status)? {
                return Ok(());
            }
        }
        self.ensure_net_start_ack()?;
        Ok(())
    }

    fn should_skip_net_start_if_ready(&mut self) -> Result<bool> {
        if query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| is_ready(status, true))
        {
            self.logger
                .info("net_start: skip NET START because network is already ready");
            return Ok(true);
        }
        Ok(false)
    }

    fn ensure_listener_on(&mut self) -> Result<()> {
        if let Err(err) = wait_net_ack(&mut self.console, "NET LISTENER ON") {
            if !self.is_listener_ready()? {
                return Err(anyhow!("net_start: listener enable failed ({err})"));
            }
            self.logger.info(format!(
                "net_start: listener enable ack not obtained ({err}); continuing because listener is already ready"
            ));
        }
        Ok(())
    }

    fn is_listener_ready(&mut self) -> Result<bool> {
        Ok(query_net_status(&mut self.console)?
            .as_ref()
            .is_some_and(|status| {
                matches!(status.state.as_deref(), Some("Ready"))
                    && status.link.unwrap_or(false)
                    && status.listener.unwrap_or(false)
                    && status.listener_enabled.unwrap_or(true)
                    && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0")
            }))
    }

    fn listener_ready_grace_ms(&self) -> Result<u32> {
        env_utils::parse_env_u32("HOSTCTL_NET_LISTENER_READY_GRACE_MS", 2_000)
    }

    fn wait_listener_ready_grace(&mut self, grace_ms: u32) -> Result<bool> {
        if grace_ms == 0 {
            return Ok(false);
        }
        let deadline = Instant::now() + Duration::from_millis(grace_ms as u64);
        while Instant::now() < deadline {
            if let Some(status) = query_net_status(&mut self.console)? {
                if is_ready(&status, true) {
                    return Ok(true);
                }
                if !is_ready_without_listener(&status) {
                    return Ok(false);
                }
            }
            thread::sleep(Duration::from_millis(150));
        }
        Ok(false)
    }

    fn should_return_from_status(&mut self, status: NetStatus) -> Result<bool> {
        if is_ready(&status, true) {
            self.logger
                .info("net_start: skip NET START because listener is already ready");
            return Ok(true);
        }
        if is_ready_without_listener(&status) {
            let grace_ms = self.listener_ready_grace_ms()?;
            if self.wait_listener_ready_grace(grace_ms)? {
                self.logger.info(format!(
                    "net_start: listener became ready within grace window ({} ms); skipping forced recover",
                    grace_ms
                ));
                return Ok(true);
            }
            self.force_recover_before_start(
                "state=Ready with listener=false while listener gate is enabled".to_string(),
            )?;
            return Ok(false);
        }
        if matches!(status.state.as_deref(), Some("Ready"))
            && status.link.unwrap_or(false)
            && status.ipv4.as_deref().is_some_and(|ip| ip != "0.0.0.0")
            && !status.listener_enabled.unwrap_or(true)
        {
            self.logger.info(
                "net_start: listener gate is disabled while network is Ready; forcing NET LISTENER ON",
            );
            self.ensure_listener_on()?;
            return Ok(true);
        }
        if self.is_stuck_listener_wait(&status) {
            self.force_recover_before_start(format!(
                "state={} with ipv4=0.0.0.0",
                status.state.as_deref().unwrap_or("unknown")
            ))?;
            return Ok(false);
        }
        if should_force_recover_before_start(&status) {
            self.force_recover_before_start(format!(
                "state={} is transitional/non-ready",
                status.state.as_deref().unwrap_or("unknown")
            ))?;
            return Ok(false);
        }
        Ok(false)
    }

    fn force_recover_before_start(&mut self, reason: String) -> Result<()> {
        self.logger.info(format!(
            "net_start: {reason}; forcing NET RECOVER before NET START"
        ));
        if env_utils::parse_env_bool01("HOSTCTL_NET_FORCE_STOP_BEFORE_RECOVER", true)? {
            if let Err(err) = wait_net_ack(&mut self.console, "NET STOP") {
                self.logger.info(format!(
                    "net_start: NET STOP ack not obtained ({err}); continuing with NET RECOVER"
                ));
            } else {
                thread::sleep(Duration::from_millis(150));
            }
        }
        wait_net_ack(&mut self.console, "NET RECOVER")?;
        thread::sleep(Duration::from_millis(self.policy.cooldown_ms as u64));
        Ok(())
    }

    fn is_stuck_listener_wait(&self, status: &NetStatus) -> bool {
        let ipv4_zero = status.ipv4.as_deref().is_none_or(|ip| ip == "0.0.0.0");
        matches!(status.state.as_deref(), Some("ListenerWait" | "DhcpWait")) && ipv4_zero
    }

    fn ensure_net_start_ack(&mut self) -> Result<()> {
        if let Err(err) = wait_net_ack(&mut self.console, "NET START") {
            if !query_net_status(&mut self.console)?.is_some_and(|status| is_ready(&status, true)) {
                return Err(err);
            }
            self.logger.info(format!(
                "net_start: start ack not obtained ({err}); continuing because network is already ready"
            ));
        }
        Ok(())
    }

    fn handle_init_wait_ready_recovery(&mut self, context: &mut Value) -> Result<()> {
        let recover_retries =
            env_utils::parse_env_u32("HOSTCTL_NET_WAIT_READY_RECOVER_RETRIES", 1)?;
        ctx_set_u32(context, "net_wait_ready_attempt", 0)?;
        ctx_set_u32(context, "net_wait_ready_recover_retries", recover_retries)?;
        ctx_set_u32(
            context,
            "net_wait_ready_loop_budget",
            recover_retries.saturating_add(1),
        )?;
        ctx_set_bool(context, "net_wait_ready_ok", false)?;
        ctx_set_bool(context, "net_wait_ready_retryable", false)?;
        ctx_set_string(context, "net_wait_ready_error", "")?;
        Ok(())
    }

    fn handle_net_wait_ready_once(&mut self, context: &mut Value) -> Result<()> {
        ctx_set_bool(context, "net_wait_ready_ok", false)?;
        ctx_set_bool(context, "net_wait_ready_retryable", false)?;

        let result = match wait_ready(&mut self.console, self.policy) {
            Ok(result) => result,
            Err(err) => {
                let detail = err.to_string();
                let retryable = should_retry_wait_ready_after_recover(&detail);
                self.logger.info(format!(
                    "net_wait_ready: {} readiness failure err={detail}",
                    if retryable {
                        "retryable"
                    } else {
                        "non-retryable"
                    }
                ));
                ctx_set_bool(context, "net_wait_ready_retryable", retryable)?;
                ctx_set_string(context, "net_wait_ready_error", &detail)?;
                ctx_set_string(context, "upload_error", &detail)?;
                return Ok(());
            }
        };

        let (mut connect_ms, mut listen_ms, mut ip) = result;
        if ctx_get_u32(context, "cycle")? == 1 {
            let (stabilized_connect_ms, stabilized_listen_ms, stabilized_ip) =
                self.enforce_startup_health_hysteresis(connect_ms, listen_ms, &ip)?;
            connect_ms = stabilized_connect_ms;
            listen_ms = stabilized_listen_ms;
            ip = stabilized_ip;
        }
        ctx_set_u32(context, "connect_ms", connect_ms)?;
        ctx_set_u32(context, "listen_ms", listen_ms)?;
        ctx_set_string(context, "ip", &ip)?;
        ctx_set_bool(context, "net_wait_ready_ok", true)?;
        ctx_set_string(context, "net_wait_ready_error", "")?;
        ctx_set_string(context, "upload_error", "")?;
        self.connect_samples.push(connect_ms as f64 / 1000.0);
        self.listen_samples.push(listen_ms as f64 / 1000.0);
        Ok(())
    }

    fn handle_increment_wait_ready_attempt(&mut self, context: &mut Value) -> Result<()> {
        let attempt = ctx_get_u32(context, "net_wait_ready_attempt")?.saturating_add(1);
        let max_retries = ctx_get_u32(context, "net_wait_ready_recover_retries")?;
        ctx_set_u32(context, "net_wait_ready_attempt", attempt)?;
        self.logger.info(format!(
            "net_wait_ready: issuing recover retry {attempt}/{max_retries}"
        ));
        Ok(())
    }
}
