use esp_hal::{
    interrupt::{InterruptHandler, Priority},
    time::Rate,
};

use crate::{task::CpuContext, TimeBase};

use super::locked::Locked;

#[cfg_attr(xtensa, path = "xtensa.rs")]
#[cfg_attr(riscv, path = "riscv.rs")]
mod arch_specific;

pub(crate) use arch_specific::*;

/// The timer responsible for time slicing.
const TIMESLICE_FREQUENCY: Rate = Rate::from_hz(crate::TICK_RATE);

static TIMER: Locked<Option<TimeBase>> = Locked::new(None);

pub(crate) fn setup_timebase(mut timer: TimeBase) {
    let cb: extern "C" fn() = unsafe { core::mem::transmute(timer_tick_handler as *const ()) };
    let handler = InterruptHandler::new(cb, Priority::Priority1);

    timer.set_interrupt_handler(handler);
    timer.listen();
    unwrap!(timer.schedule(TIMESLICE_FREQUENCY.as_duration()));
    TIMER.with(|shared: &mut Option<TimeBase>| {
        shared.replace(timer);
    });
}

pub(crate) fn clear_timer_interrupt() {
    TIMER.with(|shared: &mut Option<TimeBase>| {
        unwrap!(shared.as_mut()).clear_interrupt();
    });
}

pub(crate) fn disable_timebase() {
    TIMER.with(|shared: &mut Option<TimeBase>| {
        let mut timer = unwrap!(shared.take());
        timer.unlisten();
        timer.stop();
    });
}

extern "C" fn timer_tick_handler(_context: &mut CpuContext) {
    clear_timer_interrupt();
    TIMER.with(|shared: &mut Option<TimeBase>| {
        unwrap!(shared.as_mut()).schedule(TIMESLICE_FREQUENCY.as_duration()).ok();
    });

    cfg_if::cfg_if! {
        if #[cfg(esp32)] {
            yield_task();
        } else {
            super::task_switch(_context);
        }
    }
}
