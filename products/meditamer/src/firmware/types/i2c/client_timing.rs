//! Per-read I2C attribution, retained only for the owning acquisition cycle.
use core::cell::RefCell;
use critical_section::Mutex;

use super::wait_trace;

#[derive(Clone, Copy, Default)]
pub(crate) struct BusTiming {
    pub queue_us: u32,
    pub admission_us: u32,
    pub mutex_us: u32,
    pub config_us: u32,
    pub transfer_us: u32,
    pub attempts: u32,
    pub errors: u32,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ImuBusTiming {
    pub stages: [BusTiming; 3],
    pub waits: [WaitDetail; 2],
}

#[derive(Clone, Copy, Default)]
pub(crate) struct WaitDetail {
    pub wait_us: u32,
    pub busy_us: u32,
    pub tail_us: u32,
    pub admission_us: u32,
    pub mutex_us: u32,
    pub wake_to_admit_us: u32,
    pub task_total_us: u32,
    pub irq_total_us: u32,
    pub pipeline_prep_us: u32,
    pub pipeline_engine_us: u32,
    pub pipeline_events_us: u32,
    pub touch_phase_us: u32,
    pub transfers: u8,
    pub last_owner: u8,
    pub touch_phase: u8,
    pub woke: bool,
    pub tail_sampled: bool,
}

impl WaitDetail {
    pub(crate) const ZERO: Self = Self {
        wait_us: 0,
        busy_us: 0,
        tail_us: 0,
        admission_us: 0,
        mutex_us: 0,
        wake_to_admit_us: 0,
        task_total_us: 0,
        irq_total_us: 0,
        pipeline_prep_us: 0,
        pipeline_engine_us: 0,
        pipeline_events_us: 0,
        touch_phase_us: 0,
        transfers: 0,
        last_owner: 0,
        touch_phase: 0,
        woke: false,
        tail_sampled: false,
    };

    fn from_wait(wait: wait_trace::Snapshot) -> Self {
        Self {
            wait_us: wait.wait_us,
            busy_us: wait.busy_us,
            tail_us: wait.tail_us,
            admission_us: wait.admission_us,
            mutex_us: wait.mutex_us,
            wake_to_admit_us: wait.wake_to_admit_us,
            task_total_us: wait.tail_task_total_us,
            irq_total_us: wait.tail_irq_total_us,
            pipeline_prep_us: wait.tail_pipeline_prep_us,
            pipeline_engine_us: wait.tail_pipeline_engine_us,
            pipeline_events_us: wait.tail_pipeline_events_us,
            touch_phase_us: wait.tail_touch_phase_us,
            transfers: wait.transfers.min(u8::MAX as u32) as u8,
            last_owner: wait.addresses as u8,
            touch_phase: wait.tail_touch_phase,
            woke: wait.woke,
            tail_sampled: wait.tail_work_valid,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TouchWait {
    pub queue_us: u32,
    pub busy_us: u32,
    pub tail_us: u32,
    pub last_two: u16,
    pub transfers: u8,
    pub holder: u8,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TouchWaits {
    pub attempts: [TouchWait; 3],
    pub count: u8,
}

#[derive(Default)]
struct State {
    imu: ImuBusTiming,
    touch: BusTiming,
    touch_waits: TouchWaits,
    pending_imu_split: Option<(u32, u32)>,
    pending_touch_split: Option<(u32, u32)>,
}

static STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    imu: ImuBusTiming {
        stages: [BusTiming {
            queue_us: 0,
            admission_us: 0,
            mutex_us: 0,
            config_us: 0,
            transfer_us: 0,
            attempts: 0,
            errors: 0,
        }; 3],
        waits: [WaitDetail::ZERO; 2],
    },
    touch: BusTiming {
        queue_us: 0,
        admission_us: 0,
        mutex_us: 0,
        config_us: 0,
        transfer_us: 0,
        attempts: 0,
        errors: 0,
    },
    touch_waits: TouchWaits {
        attempts: [TouchWait {
            queue_us: 0,
            busy_us: 0,
            tail_us: 0,
            last_two: 0,
            transfers: 0,
            holder: 0,
        }; 3],
        count: 0,
    },
    pending_imu_split: None,
    pending_touch_split: None,
}));

pub(crate) fn reset_imu() {
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        state.imu = ImuBusTiming::default();
        state.pending_imu_split = None;
    });
}

pub(crate) fn imu_snapshot() -> ImuBusTiming {
    critical_section::with(|cs| STATE.borrow(cs).borrow().imu)
}

pub(crate) fn reset_touch() {
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        state.touch = BusTiming::default();
        state.touch_waits = TouchWaits::default();
        state.pending_touch_split = None;
    });
}

pub(crate) fn touch_snapshot() -> BusTiming {
    critical_section::with(|cs| STATE.borrow(cs).borrow().touch)
}

pub(crate) fn touch_wait_snapshot() -> TouchWaits {
    critical_section::with(|cs| STATE.borrow(cs).borrow().touch_waits)
}

pub(super) fn record_touch_wait(client: u8, wait: wait_trace::Snapshot) {
    if client != 1 && client != 2 {
        return;
    }
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        if client == 1 {
            state.pending_imu_split = Some((wait.admission_us, wait.mutex_us));
            let index = match wait.address {
                0x20 => Some(0),
                0x6b => Some(1),
                _ => None,
            };
            if let Some(index) = index {
                if wait.wait_us > state.imu.waits[index].wait_us {
                    state.imu.waits[index] = WaitDetail::from_wait(wait);
                }
            }
            return;
        }
        state.pending_touch_split = Some((wait.admission_us, wait.mutex_us));
        let waits = &mut state.touch_waits;
        if let Some(slot) = waits.attempts.get_mut(waits.count as usize) {
            *slot = TouchWait {
                queue_us: wait.wait_us,
                busy_us: wait.busy_us,
                tail_us: wait.tail_us,
                last_two: wait.addresses as u16,
                transfers: wait.transfers.min(u8::MAX as u32) as u8,
                holder: wait.initial_holder,
            };
            waits.count += 1;
        }
    });
}

pub(super) fn record(
    client: u8,
    address: u8,
    register: Option<u8>,
    queue_us: u32,
    config_us: u32,
    transfer_us: u32,
    error: bool,
) {
    let stage = match (address, register) {
        (0x20, Some(0x01)) => Some(0),
        (0x6b, Some(0x1c)) => Some(1),
        (0x6b, Some(0x22)) => Some(2),
        _ => None,
    };
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        let split = match client {
            1 => state.pending_imu_split.take(),
            2 => state.pending_touch_split.take(),
            _ => None,
        };
        let timing = match client {
            1 => stage.map(|stage| &mut state.imu.stages[stage]),
            2 => Some(&mut state.touch),
            _ => None,
        };
        if let Some(timing) = timing {
            timing.queue_us = timing.queue_us.saturating_add(queue_us);
            if let Some((admission_us, mutex_us)) = split {
                timing.admission_us = timing.admission_us.saturating_add(admission_us);
                timing.mutex_us = timing.mutex_us.saturating_add(mutex_us);
            }
            timing.config_us = timing.config_us.saturating_add(config_us);
            timing.transfer_us = timing.transfer_us.saturating_add(transfer_us);
            timing.attempts = timing.attempts.saturating_add(1);
            timing.errors = timing.errors.saturating_add(u32::from(error));
        }
    });
}
