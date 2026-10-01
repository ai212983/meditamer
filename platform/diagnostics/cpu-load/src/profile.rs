//! Temporary diagnostic accounting. All access is serialized by the ESP32 hook lock.
//! Times are wrapping microseconds; compare snapshots less than 71 minutes apart.
#[derive(Clone, Copy, Default)]
pub struct Counter {
    pub id: u32,
    pub calls: u32,
    pub us: u32,
}

/// IRQ identity is its array index; do not reserve another word for it.
#[derive(Clone, Copy, Default)]
pub struct IrqCounter {
    pub calls: u32,
    pub us: u32,
}

pub const TASKS: usize = 32;
pub const IRQS: usize = 132;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct TailWork {
    pub kind: u8,
    pub id: u32,
    pub us: u32,
    pub pipeline_frames: u8,
    pub pipeline_branches: u8,
    pub pipeline_poll_us: u32,
    pub touch_phase: u8,
    pub touch_phase_us: u32,
    pub task_total_us: u32,
    pub irq_total_us: u32,
    pub pipeline_prep_us: u32,
    pub pipeline_engine_us: u32,
    pub pipeline_events_us: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchPhase {
    Release = 1,
    Timing = 2,
    Classifier = 3,
    Metrics = 4,
    FrameEnqueue = 5,
    PostPublish = 6,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineBranch {
    Sample,
    Reset,
    Tick,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelinePhase {
    Prep,
    Engine,
    Events,
}
#[derive(Clone, Copy)]
struct InterruptFrame {
    irq: usize,
    task: usize,
    owner: u32,
    poll_started: u32,
}
/// A stable report contains counters and timings, not live scheduler bookkeeping.
pub struct Snapshot {
    tasks: [Counter; TASKS],
    irqs: [IrqCounter; IRQS],
    pub timings: (u32, u32, u32, u32, u32, u32, u32),
}
impl Snapshot {
    pub const fn new() -> Self {
        Self {
            tasks: [Counter {
                id: 0,
                calls: 0,
                us: 0,
            }; TASKS],
            irqs: [IrqCounter { calls: 0, us: 0 }; IRQS],
            timings: (0, 0, 0, 0, 0, 0, 0),
        }
    }
    pub fn capture(&mut self, profile: &Profile) {
        self.tasks.copy_from_slice(&profile.tasks);
        self.irqs.copy_from_slice(&profile.irqs);
        self.timings = (
            profile.at_us,
            profile.other_us,
            profile.errors,
            profile.poll_max_us,
            profile.poll_max_task,
            profile.wake_max_us,
            profile.wake_max_task,
        );
    }
    pub fn counter(&self, irq: bool, index: usize) -> Counter {
        if irq {
            let value = self.irqs[index];
            Counter {
                id: index as u32,
                calls: value.calls,
                us: value.us,
            }
        } else {
            self.tasks[index]
        }
    }
}
impl Default for Snapshot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn execution_identity_tracks_nested_interrupts_and_executor_tasks() {
        let mut live = Profile::new();
        live.thread(0, 1);
        assert_eq!(live.execution_context(), (0, 1));
        live.begin(1, 11);
        assert_eq!(live.execution_context(), (1, 11));
        live.irq_begin(2, 6);
        assert_eq!(live.execution_context(), (2, 6));
        live.begin(3, 22);
        assert_eq!(live.execution_context(), (1, 22));
        live.irq_begin(4, 7);
        assert_eq!(live.execution_context(), (2, 7));
        live.irq_end(5);
        assert_eq!(live.execution_context(), (1, 22));
        live.end(6);
        assert_eq!(live.execution_context(), (2, 6));
        live.irq_end(7);
        assert_eq!(live.execution_context(), (1, 11));
        live.thread(8, 2);
        assert_eq!(live.execution_context(), (0, 2));
        live.thread(9, 1);
        assert_eq!(live.execution_context(), (1, 11));
        live.end(10);
        assert_eq!(live.execution_context(), (0, 1));
        assert_eq!(live.errors, 0);
    }

    #[test]
    fn interrupt_task_preserves_interrupted_poll_and_accounts_time_once() {
        let mut live = Profile::new();
        live.thread(0, 1);
        live.begin(10, 11);
        live.irq_begin(30, 6);
        live.begin(35, 22);
        live.end(40);
        live.irq_end(45);
        live.end(60);
        assert_eq!(live.tasks[0].us, 35);
        // IRQ work is charged to its IRQ counter, not counted again as task time.
        assert_eq!(live.tasks[1].us, 0);
        assert_eq!(live.irqs[6].us, 15);
        assert_eq!((live.poll_max_us, live.poll_max_task), (50, 11));
        assert_eq!(live.errors, 0);
    }

    #[test]
    fn report_retains_every_irq_identity_and_is_stable_after_capture() {
        let mut live = Profile::new();
        for id in 0..IRQS {
            live.irq_begin((id * 10) as u32, id);
            live.irq_end((id * 10 + 3) as u32);
        }
        let mut report = Snapshot::new();
        report.capture(&live);
        live.irq_begin(2000, 7);
        live.irq_end(2090);
        for id in 0..IRQS {
            let counter = report.counter(true, id);
            assert_eq!((counter.id, counter.calls, counter.us), (id as u32, 1, 3));
        }
        assert_eq!(live.irqs[7].calls, 2);
        assert_eq!(report.timings.2, 0);
    }
}

#[derive(Clone, Copy)]
pub struct Profile {
    pub tasks: [Counter; TASKS],
    pub irqs: [IrqCounter; IRQS],
    pub other_us: u32,
    pub errors: u32,
    pub at_us: u32,
    /// Longest completed poll wall time, including interrupts and preemption.
    pub poll_max_us: u32,
    pub poll_max_task: u32,
    poll_started: u32,
    pub wake_max_us: u32,
    pub wake_max_task: u32,
    ready_at: [u32; TASKS],
    ready: u32,
    last: u32,
    thread: u32,
    owner: u32,
    task: usize,
    stack: [InterruptFrame; 8],
    depth: usize,
    tail_armed: bool,
    tail_best: TailWork,
    pipe_frames: u8,
    pipe_branches: u8,
    touch_phase: u8,
    touch_owner: u32,
    touch_marked: bool,
    touch_accum: u32,
    pipe_owner: u32,
    pipe_owner_valid: bool,
    pipe_stage: u8,
}
impl Profile {
    pub const fn new() -> Self {
        Self {
            tasks: [Counter {
                id: 0,
                calls: 0,
                us: 0,
            }; TASKS],
            irqs: [IrqCounter { calls: 0, us: 0 }; IRQS],
            other_us: 0,
            errors: 0,
            at_us: 0,
            poll_max_us: 0,
            poll_max_task: 0,
            poll_started: 0,
            wake_max_us: 0,
            wake_max_task: 0,
            ready_at: [0; TASKS],
            ready: 0,
            last: 0,
            thread: 0,
            owner: 0,
            task: TASKS,
            stack: [InterruptFrame {
                irq: 0,
                task: TASKS,
                owner: 0,
                poll_started: 0,
            }; 8],
            depth: 0,
            tail_armed: false,
            tail_best: TailWork {
                kind: 0,
                id: 0,
                us: 0,
                pipeline_frames: 0,
                pipeline_branches: 0,
                pipeline_poll_us: 0,
                touch_phase: 0,
                touch_phase_us: 0,
                task_total_us: 0,
                irq_total_us: 0,
                pipeline_prep_us: 0,
                pipeline_engine_us: 0,
                pipeline_events_us: 0,
            },
            pipe_frames: 0,
            pipe_branches: 0,
            touch_phase: 1,
            touch_owner: 0,
            touch_marked: false,
            touch_accum: 0,
            pipe_owner: 0,
            pipe_owner_valid: false,
            pipe_stage: 0,
        }
    }
    /// Current application execution identity: 0 RTOS thread, 1 Embassy task,
    /// 2 hardware IRQ. An interrupt entry retains the interrupted task for
    /// accounting; only a different task selected by begin() owns IRQ work.
    pub fn execution_context(&self) -> (u8, u32) {
        if self.depth != 0 {
            // SAFETY: pushes cap `depth` at the stack length, so a non-zero
            // depth leaves `depth - 1` addressing a live frame.
            let frame = *unsafe { self.stack.get_unchecked(self.depth - 1) };
            if self.task == frame.task || self.task >= TASKS {
                return (2, frame.irq as u32);
            }
        }
        if self.task < TASKS && self.owner == self.thread {
            // SAFETY: `self.task < TASKS` was just checked.
            (1, unsafe { self.tasks.get_unchecked(self.task) }.id)
        } else {
            (0, self.thread)
        }
    }

    pub fn reset_timings(&mut self, now: u32) {
        self.poll_max_us = 0;
        self.poll_max_task = 0;
        self.wake_max_us = 0;
        self.wake_max_task = 0;
        self.poll_started = now;
        self.ready_at.fill(now);
    }

    #[inline(always)]
    pub fn account(&mut self, now: u32) {
        let elapsed = now.wrapping_sub(self.last);
        if self.tail_armed {
            let (kind, id) = self.execution_context();
            if elapsed > self.tail_best.us {
                self.tail_best.kind = kind;
                self.tail_best.id = id;
                self.tail_best.us = elapsed;
            }
            if kind == 1 && id == self.touch_owner {
                let v = self.touch_accum.saturating_add(elapsed);
                self.touch_accum = v;
                if v > self.tail_best.touch_phase_us {
                    self.tail_best.touch_phase = self.touch_phase;
                    self.tail_best.touch_phase_us = v;
                }
            }
            if kind == 1 {
                self.tail_best.task_total_us = self.tail_best.task_total_us.saturating_add(elapsed);
            } else if kind == 2 {
                self.tail_best.irq_total_us = self.tail_best.irq_total_us.saturating_add(elapsed);
            }
            if kind == 1 && self.pipe_owner_valid && id == self.pipe_owner {
                match self.pipe_stage {
                    1 => {
                        self.tail_best.pipeline_prep_us =
                            self.tail_best.pipeline_prep_us.saturating_add(elapsed);
                    }
                    2 => {
                        self.tail_best.pipeline_engine_us =
                            self.tail_best.pipeline_engine_us.saturating_add(elapsed);
                    }
                    3 => {
                        self.tail_best.pipeline_events_us =
                            self.tail_best.pipeline_events_us.saturating_add(elapsed);
                    }
                    _ => {}
                }
            }
        }
        if self.depth != 0 {
            if let Some(id) = self.stack.get(self.depth - 1) {
                if let Some(c) = self.irqs.get_mut(id.irq) {
                    c.us = c.us.wrapping_add(elapsed);
                } else {
                    self.errors = self.errors.wrapping_add(1);
                }
            } else {
                self.errors = self.errors.wrapping_add(1);
            }
        } else if self.task < TASKS && self.thread == self.owner {
            // SAFETY: `self.task < TASKS` is checked in this condition.
            let slot = unsafe { self.tasks.get_unchecked_mut(self.task) };
            slot.us = slot.us.wrapping_add(elapsed);
        } else if self.thread != 0 {
            self.other_us = self.other_us.wrapping_add(elapsed);
        }
        self.last = now;
        self.at_us = now;
    }
    #[inline(always)]
    pub fn start_tail(&mut self, now: u32) {
        self.account(now);
        let (kind, id) = self.execution_context();
        self.tail_best.kind = kind;
        self.tail_best.id = id;
        self.tail_best.us = 0;
        self.tail_best.pipeline_frames = 0;
        self.tail_best.pipeline_branches = 0;
        self.tail_best.pipeline_poll_us = 0;
        self.tail_best.touch_phase = 0;
        self.tail_best.touch_phase_us = 0;
        self.tail_best.task_total_us = 0;
        self.tail_best.irq_total_us = 0;
        self.tail_best.pipeline_prep_us = 0;
        self.tail_best.pipeline_engine_us = 0;
        self.tail_best.pipeline_events_us = 0;
        self.touch_phase = 1;
        self.touch_owner = id;
        self.touch_marked = false;
        self.touch_accum = 0;
        self.pipe_owner = 0;
        self.pipe_owner_valid = false;
        self.pipe_stage = 0;
        self.tail_armed = true;
    }
    #[inline(always)]
    pub fn note_tail_touch_phase(&mut self, now: u32, phase: TouchPhase) {
        self.account(now);
        if !self.tail_armed {
            return;
        }
        let v = phase as u8;
        let (kind, id) = self.execution_context();
        if kind == 1 && !self.touch_marked && id != self.touch_owner {
            self.touch_accum = 0;
            self.tail_best.touch_phase = 0;
            self.tail_best.touch_phase_us = 0;
        }
        if kind == 1 {
            self.touch_owner = id;
        }
        self.touch_marked = true;
        self.touch_phase = v;
        self.touch_accum = 0;
    }
    #[inline(always)]
    pub fn note_tail_pipeline(&mut self, branch: PipelineBranch) {
        if !self.tail_armed {
            return;
        }
        match branch {
            PipelineBranch::Sample => {
                self.pipe_frames = self.pipe_frames.saturating_add(1);
                self.pipe_branches |= 1;
                let (kind, id) = self.execution_context();
                if kind == 1 {
                    self.pipe_owner = id;
                    self.pipe_owner_valid = true;
                }
            }
            PipelineBranch::Reset => {
                self.pipe_branches |= 2;
                self.pipe_stage = 0;
            }
            PipelineBranch::Tick => {
                self.pipe_branches |= 4;
                self.pipe_stage = 0;
            }
        }
    }
    #[inline(always)]
    pub fn note_tail_pipeline_phase(&mut self, now: u32, phase: PipelinePhase) {
        self.account(now);
        if !self.tail_armed {
            return;
        }
        let (kind, id) = self.execution_context();
        if kind == 1 {
            self.pipe_owner = id;
            self.pipe_owner_valid = true;
        }
        self.pipe_stage = match phase {
            PipelinePhase::Prep => 1,
            PipelinePhase::Engine => 2,
            PipelinePhase::Events => 3,
        };
    }
    #[inline(always)]
    pub fn stop_tail(&mut self, now: u32) -> Option<TailWork> {
        self.account(now);
        if !self.tail_armed {
            return None;
        }
        self.tail_armed = false;
        self.pipe_stage = 0;
        if !self.touch_marked {
            self.tail_best.touch_phase = 0;
            self.tail_best.touch_phase_us = 0;
        }
        Some(self.tail_best)
    }
    #[inline(always)]
    pub fn thread(&mut self, now: u32, id: u32) {
        self.account(now);
        self.thread = id;
    }
    #[inline(always)]
    pub fn begin(&mut self, now: u32, id: u32) {
        self.account(now);
        self.poll_started = now;
        self.pipe_frames = 0;
        self.pipe_branches = 0;
        self.task = TASKS;
        // Borrow both tables together so the scan needs no index bounds checks.
        for (i, (slot, ready_at)) in self
            .tasks
            .iter_mut()
            .zip(self.ready_at.iter_mut())
            .enumerate()
        {
            if slot.id == id || slot.id == 0 {
                slot.id = id;
                slot.calls = slot.calls.wrapping_add(1);
                self.task = i;
                if self.ready & (1 << i) != 0 {
                    let elapsed = now.wrapping_sub(*ready_at);
                    if elapsed > self.wake_max_us {
                        self.wake_max_us = elapsed;
                        self.wake_max_task = id;
                    }
                    self.ready &= !(1 << i);
                }
                break;
            }
        }
        if self.task == TASKS {
            self.errors = self.errors.wrapping_add(1);
        }
        self.owner = self.thread;
    }
    /// Record the first coalesced wake of a task already observed on this core.
    /// The caller searches both cores: the wake may originate on the other one.
    /// Initial spawn latency is excluded until the first poll registers the task.
    #[inline(always)]
    pub fn ready(&mut self, now: u32, id: u32) -> bool {
        if id == 0 {
            return false;
        }
        for (i, (slot, ready_at)) in self.tasks.iter().zip(self.ready_at.iter_mut()).enumerate() {
            if slot.id == id {
                if self.ready & (1 << i) == 0 {
                    *ready_at = now;
                    self.ready |= 1 << i;
                }
                return true;
            }
        }
        false
    }
    #[inline(always)]
    pub fn end(&mut self, now: u32) {
        self.account(now);
        if self.task < TASKS {
            let elapsed = now.wrapping_sub(self.poll_started);
            if elapsed > self.poll_max_us {
                self.poll_max_us = elapsed;
                // SAFETY: `self.task < TASKS` is checked in the enclosing condition.
                self.poll_max_task = unsafe { self.tasks.get_unchecked(self.task) }.id;
            }
        }
        if self.tail_armed && self.pipe_branches != 0 {
            let elapsed = now.wrapping_sub(self.poll_started);
            if self.pipe_frames > self.tail_best.pipeline_frames
                || (self.pipe_frames == self.tail_best.pipeline_frames
                    && elapsed > self.tail_best.pipeline_poll_us)
            {
                self.tail_best.pipeline_frames = self.pipe_frames;
                self.tail_best.pipeline_branches = self.pipe_branches;
                self.tail_best.pipeline_poll_us = elapsed;
            }
        }
        self.pipe_stage = 0;
        self.task = TASKS;
    }
    #[inline(always)]
    pub fn irq_begin(&mut self, now: u32, id: usize) {
        self.account(now);
        if id >= IRQS || self.depth == self.stack.len() {
            self.errors = self.errors.wrapping_add(1);
            return;
        }
        // An interrupt executor may poll another Embassy task before returning
        // to the interrupted poll. Preserve that poll's identity and start time.
        // SAFETY: the guard above returns unless `id < IRQS` and `depth`
        // is below the stack length, so both projections are in bounds.
        unsafe {
            *self.stack.get_unchecked_mut(self.depth) = InterruptFrame {
                irq: id,
                task: self.task,
                owner: self.owner,
                poll_started: self.poll_started,
            };
            self.depth += 1;
            let counter = self.irqs.get_unchecked_mut(id);
            counter.calls = counter.calls.wrapping_add(1);
        }
    }
    #[inline(always)]
    pub fn irq_end(&mut self, now: u32) {
        self.account(now);
        if self.depth == 0 {
            self.errors = self.errors.wrapping_add(1);
        } else {
            self.depth -= 1;
            // SAFETY: `depth` was non-zero and never exceeds the stack
            // length, so the decremented depth is a live frame.
            let frame = *unsafe { self.stack.get_unchecked(self.depth) };
            self.task = frame.task;
            self.owner = frame.owner;
            self.poll_started = frame.poll_started;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timing_reset_clips_in_progress_work_without_erasing_cpu_accounting() {
        let mut p = Profile::new();
        p.thread(10, 1);
        p.begin(20, 123);
        p.ready(25, 123);
        p.reset_timings(100);
        p.end(110);
        assert_eq!((p.poll_max_us, p.poll_max_task), (10, 123));
        assert_eq!(p.tasks[0].us, 90);
        p.begin(120, 123);
        assert_eq!((p.wake_max_us, p.wake_max_task), (20, 123));
        assert_eq!(p.tasks[0].calls, 2);
        assert_eq!(p.errors, 0);
    }

    #[test]
    fn excludes_waits_nested_irqs_and_other_threads() {
        let mut p = Profile::new();
        p.thread(10, 1);
        p.begin(20, 123);
        p.irq_begin(30, 129);
        p.irq_begin(35, 6);
        p.irq_end(45);
        p.irq_end(50);
        p.thread(60, 2);
        p.thread(80, 1);
        p.end(90);
        p.thread(100, 0);
        p.account(200);
        assert_eq!(p.tasks[0].us, 30);
        assert_eq!(p.irqs[129].us, 10);
        assert_eq!(p.irqs[6].us, 10);
        assert_eq!(p.other_us, 40);
        assert_eq!(p.errors, 0);
        assert_eq!((p.poll_max_us, p.poll_max_task), (70, 123));
    }

    #[test]
    fn poll_maximum_handles_clock_wrap_and_excludes_time_between_polls() {
        let mut p = Profile::new();
        p.thread(u32::MAX - 20, 1);
        p.begin(u32::MAX - 10, 123);
        p.end(9);
        assert_eq!((p.poll_max_us, p.poll_max_task), (20, 123));
        p.begin(1_000, 456);
        p.end(1_005);
        assert_eq!((p.poll_max_us, p.poll_max_task), (20, 123));
        p.begin(2_000, 456);
        p.end(2_050);
        assert_eq!((p.poll_max_us, p.poll_max_task), (50, 456));
    }

    #[test]
    fn out_of_range_irq_id_counts_error_and_leaves_state_untouched() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        // Boundary id is valid; the first invalid id is IRQS.
        p.irq_begin(20, IRQS - 1);
        p.irq_end(30);
        assert_eq!(p.irqs[IRQS - 1].calls, 1);
        assert_eq!(p.errors, 0);
        for bad in [IRQS, IRQS + 1, usize::MAX] {
            p.irq_begin(40, bad);
        }
        // Rejected entries never push a frame and never touch counters.
        assert_eq!(p.errors, 3);
        assert_eq!(p.irqs[IRQS - 1].calls, 1);
        assert_eq!(p.execution_context(), (1, 11));
        // A valid IRQ still nests and unwinds normally afterwards.
        p.irq_begin(50, 6);
        assert_eq!(p.execution_context(), (2, 6));
        p.irq_end(60);
        assert_eq!(p.execution_context(), (1, 11));
        assert_eq!(p.errors, 3);
    }

    #[test]
    fn exhausted_irq_stack_rejects_push_and_preserves_nesting() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        for depth in 0..8 {
            p.irq_begin(20 + depth as u32, depth);
        }
        assert_eq!(p.errors, 0);
        // Ninth nested entry overflows the 8-frame stack: counted, not stored.
        p.irq_begin(100, 7);
        assert_eq!(p.errors, 1);
        assert_eq!(p.execution_context(), (2, 7));
        for depth in (0..8).rev() {
            p.irq_end(110 + depth as u32);
            assert_eq!(p.errors, 1);
        }
        assert_eq!(p.execution_context(), (1, 11));
        // Unbalanced pop with an empty stack counts exactly one more error.
        p.irq_end(200);
        assert_eq!(p.errors, 2);
        assert_eq!(p.execution_context(), (1, 11));
        for id in 0..8 {
            assert_eq!(p.irqs[id].calls, 1);
        }
    }

    #[test]
    fn saturated_task_table_counts_error_and_keeps_existing_slots() {
        let mut p = Profile::new();
        p.thread(0, 1);
        for i in 0..TASKS {
            p.begin(10 + i as u32, 100 + i as u32);
            p.end(20 + i as u32);
        }
        assert_eq!(p.errors, 0);
        // Table is full of distinct ids: one more distinct task overflows.
        p.begin(1_000, 999);
        assert_eq!(p.errors, 1);
        // Re-polling a resident id still hits its slot with no new error.
        p.begin(1_010, 100);
        p.end(1_020);
        assert_eq!(p.errors, 1);
        assert_eq!(p.tasks[0].calls, 2);
        assert_eq!(p.tasks[0].id, 100);
    }

    #[test]
    fn wake_latency_retains_first_wake_and_handles_self_wake_and_wrap() {
        let mut p = Profile::new();
        assert!(!p.ready(0, 123));
        p.begin(10, 123);
        assert!(p.ready(15, 123));
        p.end(20);
        assert!(p.ready(25, 123));
        p.begin(30, 123);
        p.end(35);
        assert_eq!((p.wake_max_us, p.wake_max_task), (15, 123));
        p.begin(100, 123);
        p.end(110);
        assert_eq!(p.wake_max_us, 15);
        p.ready(u32::MAX - 10, 123);
        p.begin(20, 123);
        assert_eq!(p.wake_max_us, 31);
    }
}

#[cfg(test)]
mod tail_tests {
    use super::*;
    #[test]
    fn tail_reports_longest_task_segment_and_idle_stop_is_none() {
        let mut p = Profile::new();
        assert!(p.stop_tail(0).is_none());
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.account(70);
        p.begin(90, 22);
        p.account(100);
        let tail = p.stop_tail(130).expect("window active");
        assert_eq!((tail.kind, tail.id, tail.us), (1, 11, 50));
        assert_eq!(p.tasks[0].us, 80);
        assert_eq!(p.tasks[1].us, 40);
        assert!(p.stop_tail(140).is_none());
    }
    #[test]
    fn tail_prefers_irq_segment_over_split_task_time() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.irq_begin(40, 6);
        p.irq_end(90);
        p.end(100);
        let tail = p.stop_tail(110).expect("window active");
        assert_eq!((tail.kind, tail.id, tail.us), (2, 6, 50));
        assert_eq!(p.irqs[6].us, 50);
        assert_eq!((p.poll_max_us, p.poll_max_task), (90, 11));
    }
    #[test]
    fn tail_measures_wrapping_segment() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(u32::MAX - 50, 11);
        p.start_tail(u32::MAX - 40);
        p.account(30);
        let tail = p.stop_tail(60).expect("window active");
        assert_eq!((tail.kind, tail.id, tail.us), (1, 11, 71));
    }
    #[test]
    fn tail_pipeline_retains_highest_frame_poll_with_duration_tiebreak() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.begin(100, 11);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.end(110);
        p.begin(200, 11);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.end(230);
        let tail = p.stop_tail(240).expect("window active");
        assert_eq!(tail.pipeline_frames, 3);
        assert_eq!(tail.pipeline_branches, 1);
        assert_eq!(tail.pipeline_poll_us, 10);
        let mut q = Profile::new();
        q.thread(0, 1);
        q.begin(10, 11);
        q.start_tail(20);
        q.begin(100, 11);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.end(110);
        q.begin(200, 11);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.end(230);
        let tie = q.stop_tail(240).expect("window active");
        assert_eq!(tie.pipeline_frames, 2);
        assert_eq!(tie.pipeline_branches, 1);
        assert_eq!(tie.pipeline_poll_us, 30);
    }
    #[test]
    fn tail_pipeline_captures_branches_and_ignores_disarmed_notes() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.note_tail_pipeline(PipelineBranch::Reset);
        p.start_tail(20);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.end(30);
        let tail = p.stop_tail(40).expect("window active");
        assert_eq!(tail.pipeline_frames, 1);
        assert_eq!(tail.pipeline_branches, 1);
        assert_eq!(tail.pipeline_poll_us, 20);
        let mut q = Profile::new();
        q.thread(0, 1);
        q.begin(10, 11);
        q.start_tail(20);
        q.begin(100, 11);
        q.note_tail_pipeline(PipelineBranch::Reset);
        q.note_tail_pipeline(PipelineBranch::Tick);
        q.end(120);
        let masked = q.stop_tail(130).expect("window active");
        assert_eq!(masked.pipeline_frames, 0);
        assert_eq!(masked.pipeline_branches, 6);
        assert_eq!(masked.pipeline_poll_us, 20);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.start_tail(140);
        q.begin(150, 11);
        q.end(160);
        let cleared = q.stop_tail(170).expect("window active");
        assert_eq!(cleared.pipeline_frames, 0);
        assert_eq!(cleared.pipeline_branches, 0);
        assert_eq!(cleared.pipeline_poll_us, 0);
        assert!(q.stop_tail(180).is_none());
    }
    #[test]
    fn tail_touch_phase_counts_owner_task_only_and_retains_largest() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.account(50);
        p.irq_begin(50, 6);
        p.account(70);
        p.irq_end(70);
        p.begin(70, 22);
        p.account(90);
        p.begin(90, 11);
        p.note_tail_touch_phase(90, TouchPhase::Timing);
        p.account(130);
        p.note_tail_touch_phase(130, TouchPhase::Metrics);
        p.account(140);
        p.irq_begin(140, 6);
        p.account(150);
        p.irq_end(150);
        p.account(160);
        let tail = p.stop_tail(170).expect("window active");
        assert_eq!((tail.touch_phase, tail.touch_phase_us), (2, 40));
        assert_eq!(p.irqs[6].us, 30);
        assert_eq!(p.tasks[1].us, 20);
        let mut q = Profile::new();
        q.thread(0, 1);
        q.begin(10, 11);
        q.start_tail(20);
        q.account(80);
        q.begin(80, 22);
        q.note_tail_touch_phase(80, TouchPhase::Timing);
        q.account(90);
        let moved = q.stop_tail(90).expect("window active");
        assert_eq!((moved.touch_phase, moved.touch_phase_us), (2, 10));
        let mut r = Profile::new();
        r.thread(0, 1);
        r.begin(10, 33);
        r.start_tail(20);
        let unmarked = r.stop_tail(50).expect("window active");
        assert_eq!((unmarked.touch_phase, unmarked.touch_phase_us), (0, 0));
    }
    #[test]
    fn tail_totals_exclude_idle() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.account(50);
        p.thread(60, 0);
        p.account(80);
        p.thread(90, 1);
        p.account(100);
        let tail = p.stop_tail(110).expect("window active");
        assert_eq!(tail.task_total_us, 60);
        assert_eq!(tail.irq_total_us, 0);
        assert_eq!((tail.kind, tail.id, tail.us), (1, 11, 30));
    }
    #[test]
    fn tail_totals_split_irq_preemption() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.account(40);
        p.irq_begin(40, 6);
        p.account(70);
        p.irq_end(70);
        p.account(90);
        let tail = p.stop_tail(100).expect("window active");
        assert_eq!(tail.task_total_us, 50);
        assert_eq!(tail.irq_total_us, 30);
        assert_eq!((tail.kind, tail.id, tail.us), (2, 6, 30));
        assert_eq!(p.irqs[6].us, 30);
    }
    #[test]
    fn tail_pipeline_stage_buckets_exclude_irq() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.start_tail(20);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.note_tail_pipeline_phase(20, PipelinePhase::Prep);
        p.account(40);
        p.irq_begin(40, 6);
        p.account(60);
        p.irq_end(60);
        p.note_tail_pipeline_phase(60, PipelinePhase::Engine);
        p.account(80);
        p.note_tail_pipeline_phase(80, PipelinePhase::Events);
        p.account(100);
        let tail = p.stop_tail(110).expect("window active");
        assert_eq!(tail.pipeline_prep_us, 20);
        assert_eq!(tail.pipeline_engine_us, 20);
        assert_eq!(tail.pipeline_events_us, 30);
        assert_eq!(tail.task_total_us, 70);
        assert_eq!(tail.irq_total_us, 20);
    }
    #[test]
    fn tail_pipeline_phase_disarmed_noop_and_poll_end_clears() {
        let mut p = Profile::new();
        p.thread(0, 1);
        p.begin(10, 11);
        p.note_tail_pipeline(PipelineBranch::Sample);
        p.note_tail_pipeline_phase(20, PipelinePhase::Prep);
        assert!(p.stop_tail(30).is_none());
        p.start_tail(30);
        p.account(50);
        p.note_tail_pipeline_phase(50, PipelinePhase::Prep);
        p.account(70);
        let tail = p.stop_tail(80).expect("window active");
        assert_eq!(tail.pipeline_prep_us, 30);
        assert_eq!(tail.task_total_us, 50);
        let mut q = Profile::new();
        q.thread(0, 1);
        q.begin(10, 11);
        q.start_tail(20);
        q.note_tail_pipeline(PipelineBranch::Sample);
        q.note_tail_pipeline_phase(20, PipelinePhase::Prep);
        q.account(40);
        q.end(50);
        q.begin(60, 11);
        q.account(80);
        let cleared = q.stop_tail(90).expect("window active");
        assert_eq!(cleared.pipeline_prep_us, 30);
        assert_eq!(cleared.pipeline_engine_us, 0);
        assert_eq!(cleared.pipeline_events_us, 0);
        let mut r = Profile::new();
        r.thread(0, 1);
        r.begin(10, 11);
        r.start_tail(20);
        r.begin(30, 11);
        r.note_tail_pipeline(PipelineBranch::Sample);
        r.note_tail_pipeline_phase(30, PipelinePhase::Prep);
        r.account(50);
        r.begin(50, 22);
        r.account(70);
        r.begin(70, 11);
        r.account(80);
        let gated = r.stop_tail(90).expect("window active");
        assert_eq!(gated.pipeline_prep_us, 40);
        assert_eq!(gated.task_total_us, 70);
    }
}
