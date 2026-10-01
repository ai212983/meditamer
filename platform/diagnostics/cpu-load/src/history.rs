use crate::Reading;

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub cores: [Option<Reading>; 2],
    pub peak_percent: [u8; 2],
    pub at_ms: u32,
}

/// Peak of the last minute's five-second averages, not an instantaneous peak.
#[derive(Clone, Copy)]
pub struct History {
    pub snapshot: Snapshot,
    times: [u32; 12],
    loads: [[u8; 2]; 12],
    count: usize,
    next: usize,
}

impl History {
    pub const fn new() -> Self {
        Self {
            snapshot: Snapshot {
                cores: [None; 2],
                peak_percent: [0; 2],
                at_ms: 0,
            },
            times: [0; 12],
            loads: [[0; 2]; 12],
            count: 0,
            next: 0,
        }
    }

    pub fn record(&mut self, now_ms: u32, cores: [Option<Reading>; 2]) {
        self.times[self.next] = now_ms;
        self.loads[self.next] = cores.map(|reading| reading.map_or(0, Reading::percent));
        self.next = (self.next + 1) % 12;
        self.count = (self.count + 1).min(12);
        let mut peak_percent = [0; 2];
        for i in 0..self.count {
            if now_ms.wrapping_sub(self.times[i]) < 60_000 {
                for (core, peak) in peak_percent.iter_mut().enumerate() {
                    *peak = (*peak).max(self.loads[i][core]);
                }
            }
        }
        self.snapshot = Snapshot {
            cores,
            peak_percent,
            at_ms: now_ms,
        };
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn peaks_expire_even_when_sampling_is_delayed() {
        let mut history = History::new();
        let busy = Some(Reading {
            elapsed_us: 100,
            busy_us: 100,
            interrupt_us: 0,
        });
        let idle = Some(Reading {
            elapsed_us: 100,
            busy_us: 0,
            interrupt_us: 0,
        });
        history.record(5_000, [busy, idle]);
        history.record(10_000, [idle, busy]);
        assert_eq!(history.snapshot.peak_percent, [100, 100]);
        history.record(65_000, [idle, idle]);
        assert_eq!(history.snapshot.peak_percent, [0, 100]);
        history.record(70_000, [idle, idle]);
        assert_eq!(history.snapshot.peak_percent, [0, 0]);
    }
}
