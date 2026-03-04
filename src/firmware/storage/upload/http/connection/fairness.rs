use core::cmp::max;

const ADAPT_MAX_LEVEL: u8 = 3;
const ADAPT_EMPTY_WAIT_TRIGGER_MS: u32 = 25;
const ADAPT_EMPTY_STREAK_TRIGGER: u32 = 3;
const ADAPT_RECOVER_READS: u32 = 8;
const ADAPT_MIN_YIELD_BYTES: usize = 4 * 1024;
const ADAPT_MIN_YIELD_READS: u32 = 8;

pub(super) struct IngressFairnessAdaptive {
    enabled: bool,
    base_yield_bytes: usize,
    base_yield_reads: u32,
    active_yield_bytes: usize,
    active_yield_reads: u32,
    level: u8,
    switches: u32,
    level_max: u8,
    empty_streak: u32,
    empty_streak_max: u32,
    nonempty_streak: u32,
}

pub(super) struct IngressFairnessAdaptiveSnapshot {
    pub(super) enabled: bool,
    pub(super) switches: u32,
    pub(super) level_max: u8,
    pub(super) empty_streak_max: u32,
}

impl IngressFairnessAdaptive {
    pub(super) fn new(enabled: bool, base_yield_bytes: usize, base_yield_reads: u32) -> Self {
        Self {
            enabled,
            base_yield_bytes,
            base_yield_reads,
            active_yield_bytes: base_yield_bytes,
            active_yield_reads: base_yield_reads,
            level: 0,
            switches: 0,
            level_max: 0,
            empty_streak: 0,
            empty_streak_max: 0,
            nonempty_streak: 0,
        }
    }

    pub(super) fn observe_read(&mut self, pre_read_queue_empty: bool, read_wait_ms: u32) {
        if pre_read_queue_empty {
            self.empty_streak = self.empty_streak.saturating_add(1);
            self.empty_streak_max = max(self.empty_streak_max, self.empty_streak);
            self.nonempty_streak = 0;

            if !self.enabled {
                return;
            }
            if read_wait_ms >= ADAPT_EMPTY_WAIT_TRIGGER_MS
                || self.empty_streak >= ADAPT_EMPTY_STREAK_TRIGGER
            {
                self.increase_level();
            }
            return;
        }

        self.empty_streak = 0;
        if !self.enabled {
            return;
        }
        self.nonempty_streak = self.nonempty_streak.saturating_add(1);
        if self.nonempty_streak >= ADAPT_RECOVER_READS {
            self.nonempty_streak = 0;
            self.decrease_level();
        }
    }

    pub(super) fn yield_bytes_target(&self) -> usize {
        self.active_yield_bytes
    }

    pub(super) fn yield_reads_target(&self) -> u32 {
        self.active_yield_reads
    }

    pub(super) fn snapshot(&self) -> IngressFairnessAdaptiveSnapshot {
        IngressFairnessAdaptiveSnapshot {
            enabled: self.enabled,
            switches: self.switches,
            level_max: self.level_max,
            empty_streak_max: self.empty_streak_max,
        }
    }

    fn increase_level(&mut self) {
        if self.level >= ADAPT_MAX_LEVEL {
            return;
        }
        self.level = self.level.saturating_add(1);
        self.level_max = max(self.level_max, self.level);
        self.switches = self.switches.saturating_add(1);
        self.apply_level_targets();
    }

    fn decrease_level(&mut self) {
        if self.level == 0 {
            return;
        }
        self.level = self.level.saturating_sub(1);
        self.switches = self.switches.saturating_add(1);
        self.apply_level_targets();
    }

    fn apply_level_targets(&mut self) {
        let shift = self.level as usize;
        let bytes_target = self.base_yield_bytes >> shift;
        let reads_target = self.base_yield_reads >> shift;
        self.active_yield_bytes = max(bytes_target, ADAPT_MIN_YIELD_BYTES);
        self.active_yield_reads = max(reads_target, ADAPT_MIN_YIELD_READS);
    }
}
