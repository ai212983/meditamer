//! Fixed-size attribution for the longest observed touch bus-lock attempt.
//! Times include executor delay. Busy time is lock ownership, not wire time.
use super::admission::QueueTrace;
use core::cell::RefCell;
use critical_section::Mutex;

#[derive(Clone, Copy, Default)]
pub(super) struct TailWork {
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

#[derive(Clone, Copy, Default)]
pub(crate) struct Snapshot {
    pub wait_us: u32,
    pub started_us: u32,
    pub finished_us: u32,
    pub address: u8,
    pub initial_holder: u8,
    pub transfers: u32,
    pub addresses: u64,
    pub busy_us: u32,
    pub tail_us: u32,
    pub tail_work_kind: u8,
    pub tail_work_id: u32,
    pub tail_work_us: u32,
    pub tail_work_valid: bool,
    pub tail_pipeline_frames: u8,
    pub tail_pipeline_branches: u8,
    pub tail_pipeline_poll_us: u32,
    pub tail_touch_phase: u8,
    pub tail_touch_phase_us: u32,
    pub tail_task_total_us: u32,
    pub tail_irq_total_us: u32,
    pub tail_pipeline_prep_us: u32,
    pub tail_pipeline_engine_us: u32,
    pub tail_pipeline_events_us: u32,
    pub at_ms: u32,
    pub acquired: bool,
    pub admission_us: u32,
    pub mutex_us: u32,
    pub wake_to_admit_us: u32,
    pub woke: bool,
    pub fifo_holder: u8,
    pub fifo_head: u8,
    pub fifo_head_age_us: u32,
    pub fifo_head_turn_us: u32,
    pub fifo_head_turned: bool,
    pub fifo_head_cancelled: bool,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct HoldSpan {
    pub address: u8,
    pub acquired_us: u32,
    pub released_us: u32,
}

impl HoldSpan {
    const EMPTY: Self = Self {
        address: 0xff,
        acquired_us: 0,
        released_us: 0,
    };
}

#[derive(Clone, Copy, Default)]
pub(crate) struct HoldSnapshot {
    pub spans: [HoldSpan; 8],
    pub count: u8,
    pub overflow: bool,
}

#[derive(Default)]
struct State {
    generation: u32,
    sequence: u32,
    addresses: u64,
    busy_us: u32,
    active: Option<(u8, u32)>,
    released_at: u32,
    worst: Snapshot,
    recent_holds: [HoldSpan; 8],
    worst_holds: HoldSnapshot,
}

pub(super) struct Ticket {
    generation: u32,
    sequence: u32,
    busy_us: u32,
    holder: u8,
    started: u32,
}

pub(super) struct WaitCompletion {
    pub admitted_us: u32,
    pub completed_us: u32,
    pub address: u8,
    pub acquired: bool,
    pub woke_at: Option<u32>,
    pub tail_work: Option<TailWork>,
    pub queue_trace: Option<QueueTrace>,
}

impl State {
    fn busy_at(&self, now: u32) -> u32 {
        self.busy_us
            .wrapping_add(self.active.map_or(0, |(_, start)| now.wrapping_sub(start)))
    }
    fn begin(&self, now: u32) -> Ticket {
        Ticket {
            generation: self.generation,
            sequence: self.sequence,
            busy_us: self.busy_at(now),
            holder: self.active.map_or(0xff, |(addr, _)| addr),
            started: now,
        }
    }
    fn finish(&mut self, ticket: Ticket, completion: WaitCompletion) -> Snapshot {
        let WaitCompletion {
            admitted_us: admitted,
            completed_us: now,
            address,
            acquired,
            woke_at,
            tail_work,
            queue_trace,
        } = completion;
        let wait_us = now.wrapping_sub(ticket.started);
        let (admission_us, mutex_us, wake_to_admit_us, woke) = if acquired {
            let admission_us = admitted.wrapping_sub(ticket.started);
            let mutex_us = now.wrapping_sub(admitted);
            match woke_at {
                Some(at) => (admission_us, mutex_us, admitted.wrapping_sub(at), true),
                None => (admission_us, mutex_us, 0, false),
            }
        } else {
            (0, 0, 0, false)
        };
        let transfers = self.sequence.wrapping_sub(ticket.sequence);
        let kept = transfers.min(8);
        let addresses = if kept == 8 {
            self.addresses
        } else {
            self.addresses & ((1u64 << (kept * 8)) - 1)
        };
        let tail_us = if acquired && transfers > 0 {
            now.wrapping_sub(self.released_at)
        } else {
            0
        };
        let tail_work = if tail_us > 0 { tail_work } else { None };
        let tail_work_valid = tail_work.is_some();
        let work = tail_work.unwrap_or_default();
        let queue = queue_trace.unwrap_or(QueueTrace {
            held_address: 0xff,
            head_address: 0xff,
            ..QueueTrace::default()
        });
        let observation = Snapshot {
            wait_us,
            started_us: ticket.started,
            finished_us: now,
            address,
            initial_holder: ticket.holder,
            transfers,
            addresses,
            busy_us: self.busy_at(now).wrapping_sub(ticket.busy_us),
            tail_us,
            tail_work_kind: work.kind,
            tail_work_id: work.id,
            tail_work_us: work.us,
            tail_work_valid,
            tail_pipeline_frames: work.pipeline_frames,
            tail_pipeline_branches: work.pipeline_branches,
            tail_pipeline_poll_us: work.pipeline_poll_us,
            tail_touch_phase: work.touch_phase,
            tail_touch_phase_us: work.touch_phase_us,
            tail_task_total_us: work.task_total_us,
            tail_irq_total_us: work.irq_total_us,
            tail_pipeline_prep_us: work.pipeline_prep_us,
            tail_pipeline_engine_us: work.pipeline_engine_us,
            tail_pipeline_events_us: work.pipeline_events_us,
            at_ms: now / 1000,
            acquired,
            admission_us,
            mutex_us,
            wake_to_admit_us,
            woke,
            fifo_holder: queue.held_address,
            fifo_head: queue.head_address,
            fifo_head_age_us: queue.head_age_us,
            fifo_head_turn_us: queue.head_turn_us,
            fifo_head_turned: queue.head_turned,
            fifo_head_cancelled: queue.head_cancelled,
        };
        if ticket.generation == self.generation && address == 0x15 && wait_us > self.worst.wait_us {
            self.worst = observation;
            let count = transfers.min(8) as usize;
            let first = self.sequence.wrapping_sub(count as u32);
            let mut holds = HoldSnapshot {
                spans: [HoldSpan::EMPTY; 8],
                count: count as u8,
                overflow: transfers > 8,
            };
            for (index, span) in holds.spans.iter_mut().take(count).enumerate() {
                *span = self.recent_holds[first.wrapping_add(index as u32) as usize % 8];
            }
            self.worst_holds = holds;
        }
        observation
    }
    fn release(&mut self, now: u32) {
        if let Some((address, start)) = self.active.take() {
            self.busy_us = self.busy_us.wrapping_add(now.wrapping_sub(start));
            self.recent_holds[self.sequence as usize % 8] = HoldSpan {
                address,
                acquired_us: start,
                released_us: now,
            };
            self.sequence = self.sequence.wrapping_add(1);
            self.addresses = (self.addresses << 8) | address as u64;
            self.released_at = now;
        }
    }
}

static TRACE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    generation: 0,
    sequence: 0,
    addresses: 0,
    busy_us: 0,
    active: None,
    released_at: 0,
    worst: Snapshot {
        wait_us: 0,
        started_us: 0,
        finished_us: 0,
        address: 0,
        initial_holder: 0xff,
        transfers: 0,
        addresses: 0,
        busy_us: 0,
        tail_us: 0,
        tail_work_kind: 0,
        tail_work_id: 0,
        tail_work_us: 0,
        tail_work_valid: false,
        tail_pipeline_frames: 0,
        tail_pipeline_branches: 0,
        tail_pipeline_poll_us: 0,
        tail_touch_phase: 0,
        tail_touch_phase_us: 0,
        tail_task_total_us: 0,
        tail_irq_total_us: 0,
        tail_pipeline_prep_us: 0,
        tail_pipeline_engine_us: 0,
        tail_pipeline_events_us: 0,
        at_ms: 0,
        acquired: false,
        admission_us: 0,
        mutex_us: 0,
        wake_to_admit_us: 0,
        woke: false,
        fifo_holder: 0xff,
        fifo_head: 0xff,
        fifo_head_age_us: 0,
        fifo_head_turn_us: 0,
        fifo_head_turned: false,
        fifo_head_cancelled: false,
    },
    recent_holds: [HoldSpan::EMPTY; 8],
    worst_holds: HoldSnapshot {
        spans: [HoldSpan::EMPTY; 8],
        count: 0,
        overflow: false,
    },
}));
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    critical_section::with(|cs| f(&mut TRACE.borrow(cs).borrow_mut()))
}
pub(super) fn begin(now: u32) -> Ticket {
    with(|s| s.begin(now))
}
pub(super) fn finish(ticket: Ticket, completion: WaitCompletion) -> Snapshot {
    with(|s| s.finish(ticket, completion))
}
pub(super) fn acquired(address: u8, now: u32) {
    with(|s| s.active = Some((address, now)));
}
pub(super) fn release(now: u32) {
    with(|s| s.release(now));
}
pub(super) fn reset() {
    with(|s| {
        s.generation = s.generation.wrapping_add(1);
        s.worst = Snapshot {
            initial_holder: 0xff,
            fifo_holder: 0xff,
            fifo_head: 0xff,
            ..Snapshot::default()
        };
        s.worst_holds = HoldSnapshot::default();
    });
}
pub(crate) fn snapshot() -> (Snapshot, HoldSnapshot) {
    with(|s| (s.worst, s.worst_holds))
}

pub(crate) fn holder_snapshot(now: u32) -> (u8, u32) {
    with(|s| {
        s.active.map_or((0xff, 0), |(address, since)| {
            (address, now.wrapping_sub(since))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn holder_snapshot_reports_idle_active_age_and_clock_wrap_without_releasing() {
        with(|s| s.active = None);
        assert_eq!(holder_snapshot(100), (0xff, 0));
        with(|s| s.active = Some((0x51, 100)));
        assert_eq!(holder_snapshot(140), (0x51, 40));
        assert_eq!(holder_snapshot(150), (0x51, 50));
        with(|s| s.active = Some((0x15, u32::MAX - 10)));
        assert_eq!(holder_snapshot(10), (0x15, 21));
        with(|s| s.active = None);
        assert_eq!(holder_snapshot(20), (0xff, 0));
    }
    #[test]
    fn separates_occupied_time_from_post_release_delay() {
        let mut s = State {
            active: Some((0x6a, 90)),
            ..Default::default()
        };
        let t = s.begin(100);
        s.release(200);
        s.active = Some((0x48, 210));
        s.release(300);
        s.finish(
            t,
            WaitCompletion {
                admitted_us: 150,
                completed_us: 350,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: Some(TailWork {
                    kind: 1,
                    id: 123,
                    us: 150,
                    pipeline_frames: 2,
                    pipeline_branches: 1,
                    pipeline_poll_us: 180,
                    touch_phase: 4,
                    touch_phase_us: 80,
                    task_total_us: 200,
                    irq_total_us: 30,
                    pipeline_prep_us: 20,
                    pipeline_engine_us: 50,
                    pipeline_events_us: 10,
                }),
                queue_trace: None,
            },
        );
        let w = s.worst;
        assert_eq!((w.wait_us, w.busy_us, w.tail_us), (250, 190, 50));
        assert_eq!(
            (w.initial_holder, w.transfers, w.addresses),
            (0x6a, 2, 0x6a48)
        );
        assert_eq!(s.worst_holds.count, 2);
        assert!(!s.worst_holds.overflow);
        assert_eq!(
            (
                s.worst_holds.spans[0].address,
                s.worst_holds.spans[0].acquired_us,
                s.worst_holds.spans[0].released_us
            ),
            (0x6a, 90, 200)
        );
        assert_eq!(
            (
                s.worst_holds.spans[1].address,
                s.worst_holds.spans[1].acquired_us,
                s.worst_holds.spans[1].released_us
            ),
            (0x48, 210, 300)
        );
        assert_eq!(
            (
                w.tail_work_kind,
                w.tail_work_id,
                w.tail_work_us,
                w.tail_work_valid
            ),
            (1, 123, 150, true)
        );
        assert_eq!(
            (
                w.tail_pipeline_frames,
                w.tail_pipeline_branches,
                w.tail_pipeline_poll_us
            ),
            (2, 1, 180)
        );
        assert_eq!((w.tail_touch_phase, w.tail_touch_phase_us), (4, 80));
        assert_eq!((w.tail_task_total_us, w.tail_irq_total_us), (200, 30));
        assert_eq!(
            (
                w.tail_pipeline_prep_us,
                w.tail_pipeline_engine_us,
                w.tail_pipeline_events_us
            ),
            (20, 50, 10)
        );
    }
    #[test]
    fn reset_rejects_straddling_wait_and_times_wrap() {
        let mut s = State::default();
        let t = s.begin(1);
        s.generation += 1;
        s.finish(
            t,
            WaitCompletion {
                admitted_us: 50,
                completed_us: 100,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!(s.worst.wait_us, 0);
        assert_eq!(s.worst_holds.count, 0);
        let t = s.begin(u32::MAX - 10);
        s.finish(
            t,
            WaitCompletion {
                admitted_us: 0,
                completed_us: 10,
                address: 0x15,
                acquired: false,
                woke_at: None,
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!(s.worst.wait_us, 21);
        assert!(!s.worst.acquired);
    }
    #[test]
    fn timeout_accounts_for_still_held_bus_without_inventing_release() {
        let mut s = State {
            active: Some((0x6b, 90)),
            ..Default::default()
        };
        let t = s.begin(100);
        s.finish(
            t,
            WaitCompletion {
                admitted_us: 5000,
                completed_us: 40100,
                address: 0x15,
                acquired: false,
                woke_at: Some(200),
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!((s.worst.wait_us, s.worst.busy_us), (40000, 40000));
        assert_eq!((s.worst.transfers, s.worst.tail_us), (0, 0));
        assert_eq!(s.active, Some((0x6b, 90)));
        assert!(!s.worst.acquired);
        assert_eq!(
            (
                s.worst.admission_us,
                s.worst.mutex_us,
                s.worst.wake_to_admit_us
            ),
            (0, 0, 0)
        );
        assert!(!s.worst.woke);
    }

    #[test]
    fn history_is_bounded_to_last_eight_completed_owners() {
        let mut s = State::default();
        let t = s.begin(0);
        for address in 1..=10 {
            s.active = Some((address, address as u32));
            s.release(address as u32 + 1);
        }
        s.finish(
            t,
            WaitCompletion {
                admitted_us: 40,
                completed_us: 100,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!(s.worst.transfers, 10);
        assert_eq!(s.worst.addresses, 0x030405060708090a);
        assert_eq!(s.worst_holds.count, 8);
        assert!(s.worst_holds.overflow);
        assert_eq!(s.worst_holds.spans[0].address, 3);
        assert_eq!(s.worst_holds.spans[7].address, 10);
    }
    #[test]
    fn immediate_admission_splits_fifo_and_mutex() {
        let mut s = State::default();
        let t = s.begin(1000);
        let w = s.finish(
            t,
            WaitCompletion {
                admitted_us: 1000,
                completed_us: 1010,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!((w.wait_us, w.admission_us, w.mutex_us), (10, 0, 10));
        assert_eq!((w.wake_to_admit_us, w.woke), (0, false));
        assert_eq!(w.admission_us.wrapping_add(w.mutex_us), w.wait_us);
        assert!(w.acquired);
    }
    #[test]
    fn queued_wake_reports_wake_to_admit() {
        let mut s = State::default();
        let t = s.begin(1000);
        let w = s.finish(
            t,
            WaitCompletion {
                admitted_us: 1250,
                completed_us: 1300,
                address: 0x15,
                acquired: true,
                woke_at: Some(1200),
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!((w.wait_us, w.admission_us, w.mutex_us), (300, 250, 50));
        assert_eq!((w.wake_to_admit_us, w.woke), (50, true));
        assert_eq!(w.admission_us.wrapping_add(w.mutex_us), w.wait_us);
    }
    #[test]
    fn split_survives_u32_wrap() {
        let mut s = State::default();
        let t = s.begin(u32::MAX - 10);
        let w = s.finish(
            t,
            WaitCompletion {
                admitted_us: 5,
                completed_us: 10,
                address: 0x15,
                acquired: true,
                woke_at: Some(u32::MAX - 5),
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!((w.wait_us, w.admission_us, w.mutex_us), (21, 16, 5));
        assert_eq!((w.wake_to_admit_us, w.woke), (11, true));
        assert_eq!(w.admission_us.wrapping_add(w.mutex_us), w.wait_us);
    }
    #[test]
    fn timeout_clears_split() {
        let mut s = State::default();
        let t = s.begin(100);
        let w = s.finish(
            t,
            WaitCompletion {
                admitted_us: 500,
                completed_us: 40100,
                address: 0x15,
                acquired: false,
                woke_at: Some(200),
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!(w.wait_us, 40000);
        assert_eq!((w.admission_us, w.mutex_us, w.wake_to_admit_us), (0, 0, 0));
        assert!(!w.woke);
        assert!(!w.acquired);
    }
    #[test]
    fn worst_wait_keeps_its_own_fifo_head_outcome() {
        let mut s = State::default();
        let first = s.begin(100);
        s.finish(
            first,
            WaitCompletion {
                admitted_us: 200,
                completed_us: 300,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: Some(QueueTrace {
                    held_address: 0xff,
                    head_address: 0x6b,
                    head_age_us: 80,
                    head_turn_us: 120,
                    head_turned: true,
                    head_cancelled: false,
                }),
            },
        );
        let second = s.begin(400);
        s.finish(
            second,
            WaitCompletion {
                admitted_us: 450,
                completed_us: 500,
                address: 0x15,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: Some(QueueTrace {
                    held_address: 0x48,
                    head_address: 0xff,
                    ..QueueTrace::default()
                }),
            },
        );
        assert_eq!(s.worst.wait_us, 200);
        assert_eq!(s.worst.fifo_holder, 0xff);
        assert_eq!(s.worst.fifo_head, 0x6b);
        assert_eq!(s.worst.fifo_head_age_us, 80);
        assert_eq!(s.worst.fifo_head_turn_us, 120);
        assert!(s.worst.fifo_head_turned);
        assert!(!s.worst.fifo_head_cancelled);
        let imu = s.begin(600);
        s.finish(
            imu,
            WaitCompletion {
                admitted_us: 2000,
                completed_us: 2500,
                address: 0x6b,
                acquired: true,
                woke_at: None,
                tail_work: None,
                queue_trace: None,
            },
        );
        assert_eq!(s.worst.wait_us, 200);
    }
}
