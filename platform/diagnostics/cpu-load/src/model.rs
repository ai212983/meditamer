#[derive(Clone, Copy)]
pub enum Event {
    Task,
    Idle,
    InterruptEnter,
    InterruptExit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub elapsed_us: u32,
    pub busy_us: u32,
    pub interrupt_us: u32,
}

impl Reading {
    pub fn percent(self) -> u8 {
        if self.elapsed_us == 0 {
            return 0;
        }
        ((u64::from(self.busy_us) * 100 + u64::from(self.elapsed_us) / 2)
            / u64::from(self.elapsed_us))
        .min(100) as u8
    }
}

/// All timestamps are wrapping microseconds from one shared monotonic clock.
/// Call sample at least once per 2^32 microseconds (about 71 minutes).
#[derive(Clone, Copy)]
pub struct Core {
    initialized: bool,
    task: bool,
    depth: u32,
    last_us: u32,
    window_us: u32,
    busy_us: u32,
    interrupt_us: u32,
}

impl Core {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            task: false,
            depth: 0,
            last_us: 0,
            window_us: 0,
            busy_us: 0,
            interrupt_us: 0,
        }
    }

    #[inline(always)]
    fn account(&mut self, now_us: u32) {
        if !self.initialized {
            self.initialized = true;
            self.window_us = now_us;
        } else {
            let elapsed = now_us.wrapping_sub(self.last_us);
            if self.task || self.depth != 0 {
                self.busy_us = self.busy_us.wrapping_add(elapsed);
            }
            if self.depth != 0 {
                self.interrupt_us = self.interrupt_us.wrapping_add(elapsed);
            }
        }
        self.last_us = now_us;
    }

    #[inline(always)]
    pub fn event(&mut self, now_us: u32, event: Event) {
        self.account(now_us);
        match event {
            Event::Task => self.task = true,
            Event::Idle => self.task = false,
            Event::InterruptEnter => self.depth = self.depth.saturating_add(1),
            Event::InterruptExit => self.depth = self.depth.saturating_sub(1),
        }
    }

    pub fn sample(&mut self, now_us: u32) -> Option<Reading> {
        if !self.initialized {
            return None;
        }
        self.account(now_us);
        let reading = Reading {
            elapsed_us: now_us.wrapping_sub(self.window_us),
            busy_us: self.busy_us,
            interrupt_us: self.interrupt_us,
        };
        self.window_us = now_us;
        self.busy_us = 0;
        self.interrupt_us = 0;
        Some(reading)
    }
}

impl Default for Core {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_interrupts_are_busy_and_nested_interrupts_are_not_double_counted() {
        let mut core = Core::new();
        core.event(0, Event::Idle);
        core.event(20, Event::InterruptEnter);
        core.event(30, Event::InterruptEnter);
        core.event(40, Event::InterruptExit);
        core.event(50, Event::InterruptExit);
        assert_eq!(
            core.sample(100),
            Some(Reading {
                elapsed_us: 100,
                busy_us: 30,
                interrupt_us: 30
            })
        );
    }

    #[test]
    fn switching_to_idle_inside_an_interrupt_remains_busy_until_exit() {
        let mut core = Core::new();
        core.event(0, Event::Task);
        core.event(20, Event::InterruptEnter);
        core.event(30, Event::Idle);
        core.event(40, Event::InterruptExit);
        assert_eq!(core.sample(100).unwrap().percent(), 40);
    }

    #[test]
    fn sampling_includes_open_intervals_and_handles_clock_wrap() {
        let mut core = Core::new();
        assert!(core.sample(10).is_none());
        core.event(u32::MAX - 49, Event::Task);
        assert_eq!(
            core.sample(50).unwrap(),
            Reading {
                elapsed_us: 100,
                busy_us: 100,
                interrupt_us: 0
            }
        );
        assert_eq!(core.sample(150).unwrap().percent(), 100);
        core.event(150, Event::Idle);
        assert_eq!(core.sample(250).unwrap().percent(), 0);
    }
}
