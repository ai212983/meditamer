use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::AssetResidencyRuntime;

impl AssetResidencyRuntime<'_> {
    pub(super) fn dispatch_phase_actions(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Option<Result<()>> {
        match action {
            "ready_pause" => Some(self.ready_pause()),
            "init_run" => Some(self.init_run()),
            "begin_cycle" => Some(self.begin_cycle(args)),
            "begin_phase" => Some(self.begin_phase(args)),
            _ => None,
        }
    }

    fn ready_pause(&mut self) -> Result<()> {
        std::thread::sleep(Duration::from_secs(1));
        Ok(())
    }

    fn init_run(&mut self) -> Result<()> {
        self.phase_name = "boot".to_string();
        self.phase_mark = self.run_mark;
        Ok(())
    }

    fn begin_cycle(&mut self, args: &Value) -> Result<()> {
        let index = args
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("begin_cycle requires numeric index"))?;
        let cycle = u8::try_from(index + 1)
            .map_err(|_| anyhow!("begin_cycle index out of range: {index}"))?;
        self.active_cycle = Some(cycle);
        self.phase_name = format!("cycle_{cycle:02}");
        self.phase_mark = self.console.mark();
        self.logger
            .info(format!("asset residency reentry cycle {cycle} started"));
        Ok(())
    }

    fn begin_phase(&mut self, args: &Value) -> Result<()> {
        let name =
            super::arg_str(args, "name").ok_or_else(|| anyhow!("begin_phase requires name"))?;
        if name.trim().is_empty() {
            return Err(anyhow!("begin_phase requires a non-empty name"));
        }
        self.phase_name = name.to_string();
        self.phase_mark = self.console.mark();
        Ok(())
    }
}
