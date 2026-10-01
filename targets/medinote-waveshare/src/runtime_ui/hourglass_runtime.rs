//! Hourglass physics, render cadence, LVGL servicing, and timing telemetry.
//! All LVGL access uses the checked UI token.

use embassy_time::{Duration as EmbassyDuration, Instant as EmbassyInstant};

use hourglass::backend::{ACTIVE_BACKEND, ACTIVE_GRAVITY_SCHEDULE, PARTICLE_CAPACITY};
use hourglass::model::{HourglassModel, SessionState, DEFAULT_DURATION_S, PHYSICS_HZ};
use hourglass::presentation::FrameData;
use medinote::controls::{OrderedRotationCommands, RotationInput};
use medinote::power::RuntimePowerMode;
use medinote::ui::screen::hourglass::Hourglass;
use render::lvgl_adapter::UiAccessToken;
use waveshare_rlcd42::panel_lvgl::PanelLvglSession;

use super::telemetry::RuntimeTelemetry;

/// Render at 10 Hz from the device-measured 30 Hz physics cadence. Every
/// physics step is still preserved regardless of this divisor.
const RENDER_EVERY_N_TICKS: u32 = 3;
const METRICS_INTERVAL_SECS: u64 = 30;

#[derive(Default)]
struct TimingStats {
    ticks: u32,
    renders: u32,
    inputs: u32,
    physics_max_us: u64,
    render_max_us: u64,
    work_max_us: u64,
    lateness_max_us: u64,
    missed_periods: u32,
    flush_bytes_max: usize,
    cue_count: u32,
    cue_max_us: u64,
}

impl TimingStats {
    fn record_tick(&mut self, physics_us: u64, work_us: u64, lateness_us: u64, period_us: u64) {
        self.ticks = self.ticks.saturating_add(1);
        self.physics_max_us = self.physics_max_us.max(physics_us);
        self.work_max_us = self.work_max_us.max(work_us);
        self.lateness_max_us = self.lateness_max_us.max(lateness_us);
        self.missed_periods = self
            .missed_periods
            .saturating_add(u32::from(lateness_us >= period_us));
    }

    fn record_render(&mut self, render_us: u64) {
        self.renders = self.renders.saturating_add(1);
        self.render_max_us = self.render_max_us.max(render_us);
        self.flush_bytes_max = self
            .flush_bytes_max
            .max(waveshare_rlcd42::panel::last_flush_bytes());
    }

    fn record_input(&mut self) {
        self.inputs = self.inputs.saturating_add(1);
    }

    fn record_cue(&mut self, cue_us: u64) {
        self.cue_count = self.cue_count.saturating_add(1);
        self.cue_max_us = self.cue_max_us.max(cue_us);
    }

    fn log(&self, state: SessionState) {
        console::println!(
            "HOURGLASS_METRICS state={:?} ticks={} renders={} inputs={} physics_max_us={} render_max_us={} work_max_us={} lateness_max_us={} missed_periods={} flush_bytes_max={} cue_count={} cue_max_us={}",
            state,
            self.ticks,
            self.renders,
            self.inputs,
            self.physics_max_us,
            self.render_max_us,
            self.work_max_us,
            self.lateness_max_us,
            self.missed_periods,
            self.flush_bytes_max,
            self.cue_count,
            self.cue_max_us,
        );
    }
}

pub(crate) struct HourglassRuntime {
    model: &'static mut HourglassModel,
    previous_state: SessionState,
    commands: OrderedRotationCommands,
    expected_tick: EmbassyInstant,
    next_metrics: EmbassyInstant,
    tick_index: u32,
    timing: TimingStats,
    pending_visible_input_at_us: Option<u64>,
}

impl HourglassRuntime {
    pub(crate) fn new(model: &'static mut HourglassModel) -> Self {
        let now = EmbassyInstant::now();
        let now_us = super::monotonic_micros();
        console::println!(
            "HOURGLASS_BACKEND name={} gravity_schedule={} grains={} duration_s={}",
            ACTIVE_BACKEND,
            ACTIVE_GRAVITY_SCHEDULE,
            PARTICLE_CAPACITY,
            DEFAULT_DURATION_S,
        );
        Self {
            previous_state: model.state(),
            model,
            commands: OrderedRotationCommands::new(now_us),
            expected_tick: now,
            next_metrics: now + EmbassyDuration::from_secs(METRICS_INTERVAL_SECS),
            tick_index: 0,
            timing: TimingStats::default(),
            pending_visible_input_at_us: None,
        }
    }

    pub(crate) fn activate(&mut self) {
        let now = EmbassyInstant::now();
        self.previous_state = self.model.state();
        self.commands.reset(super::monotonic_micros());
        self.expected_tick = now;
        self.next_metrics = now + EmbassyDuration::from_secs(METRICS_INTERVAL_SECS);
        self.tick_index = 0;
        self.timing = TimingStats::default();
        self.pending_visible_input_at_us = None;
    }

    pub(crate) fn tick(
        &mut self,
        ui_token: &UiAccessToken,
        widget: &Hourglass,
        inputs: &[RotationInput],
        panel_session: &mut PanelLvglSession,
        telemetry: &mut RuntimeTelemetry,
    ) -> RuntimePowerMode {
        let tick_started = EmbassyInstant::now();
        let lateness_us = tick_started
            .saturating_duration_since(self.expected_tick)
            .as_micros();
        let tick_period = EmbassyDuration::from_hz(PHYSICS_HZ as u64);
        let tick_period_us = tick_period.as_micros();
        self.expected_tick += tick_period;

        for input in inputs {
            let Some(command) = self.commands.command(*input) else {
                continue;
            };
            if self.model.start() {
                self.timing.record_input();
                self.remember_visible_input(input.ticks_us);
                console::println!(
                    "HOURGLASS_START source={:?} at_us={} input_count={}",
                    input.control,
                    command.at.0,
                    self.timing.inputs
                );
                continue;
            }
            let accepted = self.model.apply_command(command).is_ok();
            if accepted {
                self.timing.record_input();
                self.remember_visible_input(input.ticks_us);
            }
            console::println!(
                "HOURGLASS_CONTROL source={:?} accepted={} at_us={} input_count={}",
                input.control,
                accepted,
                command.at.0,
                self.timing.inputs,
            );
        }

        let physics_started = EmbassyInstant::now();
        self.model.tick();
        let physics_us = physics_started.elapsed().as_micros();

        let state = self.model.state();
        let mut completed_this_tick = false;
        if state != self.previous_state {
            if state == SessionState::Complete {
                // One visual cue, emitted exactly once on the transition
                // into `Complete`: a forced full-panel refresh, distinct
                // from the ordinary partial-refresh animation cadence, plus
                // the status label's own text change to "Complete" below.
                // Completion is intentionally visual; this runtime does not
                // own an audio service.
                let cue_started = EmbassyInstant::now();
                panel_session.force_full_refresh();
                self.timing.record_cue(cue_started.elapsed().as_micros());
                completed_this_tick = true;
            }
            self.previous_state = state;
        }

        self.tick_index += 1;
        if self.tick_index.is_multiple_of(RENDER_EVERY_N_TICKS) {
            let flush_before = waveshare_rlcd42::panel::flush_completion();
            let render_started = EmbassyInstant::now();
            let frame = FrameData::capture(self.model);
            widget.render(ui_token, &frame);
            let elapsed_ms = 1000 * RENDER_EVERY_N_TICKS / PHYSICS_HZ;
            if let Ok(mut idle_ms) = ui_token.run_timer_handler(elapsed_ms) {
                for _ in 1..8 {
                    if idle_ms > 0 {
                        break;
                    }
                    let Ok(next_idle_ms) = ui_token.run_timer_handler(0) else {
                        break;
                    };
                    idle_ms = next_idle_ms;
                }
            }
            self.timing
                .record_render(render_started.elapsed().as_micros());
            let flush_after = waveshare_rlcd42::panel::flush_completion();
            if flush_before.sequence != flush_after.sequence {
                if let Some(input_at_us) = self.pending_visible_input_at_us.take() {
                    telemetry.record_input_flush(input_at_us, flush_before, flush_after);
                }
            }
        }

        self.timing.record_tick(
            physics_us,
            tick_started.elapsed().as_micros(),
            lateness_us,
            tick_period_us,
        );
        let now = EmbassyInstant::now();
        if completed_this_tick || now >= self.next_metrics {
            self.timing.log(state);
            while self.next_metrics <= now {
                self.next_metrics += EmbassyDuration::from_secs(METRICS_INTERVAL_SECS);
            }
        }

        RuntimePowerMode::for_hourglass_state(state)
    }

    fn remember_visible_input(&mut self, input_at_us: u64) {
        self.pending_visible_input_at_us = Some(
            self.pending_visible_input_at_us
                .map_or(input_at_us, |pending| pending.min(input_at_us)),
        );
    }
}
