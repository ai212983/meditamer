//! Bounded attribution of the longest healthy active start-to-start interval.
use crate::firmware::types::i2c::ImuBusTiming;
use core::cell::RefCell;
use critical_section::Mutex;
use inkplate_tempera::imu::ImuReadTiming;

#[derive(Clone, Copy, Default)]
pub(crate) struct Cycle {
    pub start_us: u64,
    pub wake_us: u32,
    pub service_us: u32,
    pub publish_us: u32,
    pub read_timing: ImuReadTiming,
    pub bus_timing: ImuBusTiming,
    pub aux_us: u32,
    pub skipped_after: u32,
    pub rebase: bool,
    pub resumed: bool,
    pub mode_changed: bool,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct Snapshot {
    pub gap_us: u32,
    pub previous_wake_us: u32,
    pub previous_service_us: u32,
    pub previous_publish_us: u32,
    pub previous_start_ms: u32,
    pub previous_read_timing: ImuReadTiming,
    pub previous_bus_timing: ImuBusTiming,
    pub previous_aux_us: u32,
    pub previous_skipped: u32,
    pub previous_rebase: bool,
    pub previous_resumed: bool,
    pub previous_mode_changed: bool,
    pub current_wake_us: u32,
    pub skipped: u32,
}
#[derive(Default)]
struct State {
    previous: Option<Cycle>,
    snapshot: Snapshot,
    pending_resumed: bool,
}
impl State {
    fn note_resumed(&mut self) {
        self.pending_resumed = true;
    }
    fn note_deadline(&mut self, rebase: bool, mode_changed: bool) {
        if let Some(previous) = self.previous.as_mut() {
            previous.rebase = rebase;
            previous.mode_changed = mode_changed;
        }
    }
    fn record(&mut self, cycle: Cycle, active: bool, discontinuity: bool) {
        let mut cycle = cycle;
        cycle.resumed |= self.pending_resumed;
        self.pending_resumed = false;
        if active && !discontinuity {
            if let Some(previous) = self.previous {
                let gap = cycle
                    .start_us
                    .saturating_sub(previous.start_us)
                    .min(u32::MAX as u64) as u32;
                if gap > self.snapshot.gap_us {
                    self.snapshot = Snapshot {
                        gap_us: gap,
                        previous_wake_us: previous.wake_us,
                        previous_service_us: previous.service_us,
                        previous_publish_us: previous.publish_us,
                        // Match I2C_WAIT's wrapping 32-bit microsecond clock.
                        previous_start_ms: (previous.start_us as u32) / 1000,
                        previous_read_timing: previous.read_timing,
                        previous_bus_timing: previous.bus_timing,
                        previous_aux_us: previous.aux_us,
                        previous_skipped: previous.skipped_after,
                        previous_rebase: previous.rebase,
                        previous_resumed: previous.resumed,
                        previous_mode_changed: previous.mode_changed,
                        current_wake_us: cycle.wake_us,
                        skipped: self.snapshot.skipped,
                    };
                }
            }
        }
        self.previous = active.then_some(cycle);
    }
}
static STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    previous: None,
    pending_resumed: false,
    snapshot: Snapshot {
        gap_us: 0,
        previous_wake_us: 0,
        previous_service_us: 0,
        previous_publish_us: 0,
        previous_start_ms: 0,
        previous_read_timing: ImuReadTiming {
            interrupt_port_us: 0,
            sensor_us: 0,
        },
        previous_bus_timing: ImuBusTiming {
            stages: [crate::firmware::types::i2c::BusTiming {
                queue_us: 0,
                admission_us: 0,
                mutex_us: 0,
                config_us: 0,
                transfer_us: 0,
                attempts: 0,
                errors: 0,
            }; 3],
            waits: [crate::firmware::types::i2c::WaitDetail::ZERO; 2],
        },
        previous_aux_us: 0,
        previous_skipped: 0,
        previous_rebase: false,
        previous_resumed: false,
        previous_mode_changed: false,
        current_wake_us: 0,
        skipped: 0,
    },
}));
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    critical_section::with(|cs| f(&mut STATE.borrow(cs).borrow_mut()))
}
pub(crate) fn record(cycle: Cycle, active: bool, discontinuity: bool) {
    with(|s| s.record(cycle, active, discontinuity));
}
pub(crate) fn note_resumed() {
    with(|s| s.note_resumed());
}
pub(crate) fn note_deadline(rebase: bool, mode_changed: bool) {
    with(|s| s.note_deadline(rebase, mode_changed));
}
pub(crate) fn skipped(count: u64) {
    with(|s| {
        if let Some(previous) = s.previous.as_mut() {
            previous.skipped_after = count.min(u32::MAX as u64) as u32;
        }
        s.snapshot.skipped = s
            .snapshot
            .skipped
            .saturating_add(count.min(u32::MAX as u64) as u32)
    });
}
pub(crate) fn snapshot() -> Snapshot {
    with(|s| s.snapshot)
}
pub(crate) fn reset() {
    with(|s| *s = State::default());
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_gap_keeps_its_own_wait_not_a_later_global_maximum() {
        let mut state = State::default();
        let mut first = Cycle {
            start_us: 1_000,
            ..Cycle::default()
        };
        first.bus_timing.waits[0].wait_us = 3_000;
        state.record(first, true, false);
        let mut second = Cycle {
            start_us: 18_000,
            ..Cycle::default()
        };
        second.bus_timing.waits[0].wait_us = 5_000;
        state.record(second, true, false);
        state.record(
            Cycle {
                start_us: 26_000,
                ..Cycle::default()
            },
            true,
            false,
        );
        assert_eq!(state.snapshot.gap_us, 17_000);
        assert_eq!(state.snapshot.previous_bus_timing.waits[0].wait_us, 3_000);
    }

    #[test]
    fn attributes_gap_to_previous_work_and_current_wake() {
        let mut s = State::default();
        s.note_resumed();
        s.record(
            Cycle {
                start_us: 1000,
                wake_us: 10,
                service_us: 2000,
                publish_us: 3000,
                read_timing: ImuReadTiming {
                    interrupt_port_us: 100,
                    sensor_us: 500,
                },
                bus_timing: ImuBusTiming::default(),
                aux_us: 400,
                skipped_after: 0,
                rebase: false,
                resumed: false,
                mode_changed: false,
            },
            true,
            false,
        );
        s.previous.as_mut().unwrap().skipped_after = 1;
        s.note_deadline(true, true);
        s.record(
            Cycle {
                start_us: 18000,
                wake_us: 100,
                service_us: 1,
                publish_us: 2,
                ..Cycle::default()
            },
            true,
            false,
        );
        assert_eq!(s.snapshot.gap_us, 17000);
        assert_eq!(s.snapshot.previous_start_ms, 1);
        assert_eq!(s.snapshot.previous_read_timing.interrupt_port_us, 100);
        assert_eq!(s.snapshot.previous_read_timing.sensor_us, 500);
        assert_eq!(s.snapshot.previous_aux_us, 400);
        assert_eq!(s.snapshot.previous_skipped, 1);
        assert_eq!(
            (
                s.snapshot.previous_service_us,
                s.snapshot.previous_publish_us,
                s.snapshot.current_wake_us
            ),
            (2000, 3000, 100)
        );
        assert!(s.snapshot.previous_rebase);
        assert!(s.snapshot.previous_resumed);
        assert!(s.snapshot.previous_mode_changed);
        s.note_deadline(false, false);
        assert!(s.snapshot.previous_rebase);
        assert!(s.snapshot.previous_resumed);
        assert!(s.snapshot.previous_mode_changed);
        let current = s.previous.as_ref().unwrap();
        assert!(!current.rebase);
        assert!(!current.resumed);
        assert!(!current.mode_changed);
        s.record(
            Cycle {
                start_us: 100000,
                ..Cycle::default()
            },
            true,
            true,
        );
        s.record(
            Cycle {
                start_us: 108000,
                ..Cycle::default()
            },
            true,
            false,
        );
        assert_eq!(s.snapshot.gap_us, 17000);
        let mut wrapped = State::default();
        wrapped.record(
            Cycle {
                start_us: u32::MAX as u64 + 1002,
                ..Cycle::default()
            },
            true,
            false,
        );
        wrapped.record(
            Cycle {
                start_us: u32::MAX as u64 + 9002,
                ..Cycle::default()
            },
            true,
            false,
        );
        assert_eq!(wrapped.snapshot.previous_start_ms, 1);
    }
}
