use core::ptr::NonNull;

use esp_hal::{system::Cpu, time::Instant};

use crate::{
    SCHEDULER,
    esp_radio::sem_trace,
    task::{TaskList, TaskPtr, TaskReadyQueueElement},
};

const ESP_RTOS_WAIT_QUEUE_NOTIFY_ONE: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_ESP_RTOS_WAIT_QUEUE_NOTIFY_ONE") {
        Some(value) => Some(value),
        None => option_env!("ESP_RTOS_WAIT_QUEUE_NOTIFY_ONE"),
    },
);

const fn parse_nonzero_flag(value: Option<&'static str>) -> bool {
    match value {
        Some(value) => {
            let bytes = value.as_bytes();
            !(bytes.is_empty() || (bytes.len() == 1 && bytes[0] == b'0'))
        }
        None => false,
    }
}

pub(crate) struct WaitQueue {
    // A task is either blocked, or ready. Since it can't be both, we can reuse the ready queue
    // element. Note however, that a task can simultaneously be in the timer queue and a wait
    // queue!
    pub(crate) waiting_tasks: TaskList<TaskReadyQueueElement>,
}

impl WaitQueue {
    pub(crate) const fn new() -> Self {
        Self {
            waiting_tasks: TaskList::new(),
        }
    }

    pub(crate) fn notify(&mut self) {
        SCHEDULER.with(|scheduler| {
            let queue_ptr = self as *mut Self as usize;
            if ESP_RTOS_WAIT_QUEUE_NOTIFY_ONE {
                let mut resumed = 0usize;
                if let Some(waken_task) = self.waiting_tasks.pop() {
                    scheduler.resume_task(waken_task);
                    resumed = 1;
                }
                sem_trace::trace_wait_queue_notify(queue_ptr, resumed);
                return;
            }

            let mut resumed = 0usize;
            while let Some(waken_task) = self.waiting_tasks.pop() {
                scheduler.resume_task(waken_task);
                resumed += 1;
            }
            sem_trace::trace_wait_queue_notify(queue_ptr, resumed);
        });
    }

    pub(crate) fn wait_with_deadline(&mut self, deadline: Instant) {
        SCHEDULER.with(|scheduler| {
            let mut task = scheduler.current_task(Cpu::current());
            if scheduler.sleep_task_until(task, deadline) {
                sem_trace::trace_wait_queue_sleep(self as *mut Self as usize);
                self.waiting_tasks.push(task);
                unsafe {
                    task.as_mut().current_queue = Some(NonNull::from(self));
                }
                crate::task::yield_task();
            }
        });
    }

    pub(crate) fn remove(&mut self, task: TaskPtr) {
        self.waiting_tasks.remove(task);
    }
}
