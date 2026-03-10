use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use portable_atomic::{AtomicUsize, Ordering};
use esp_sync::RawMutex;

use crate::{
    run_queue::RunQueue,
    task::{TaskExt, TaskPtr, TaskState},
    SCHEDULER,
};

const MAX_TASKS: usize = 8;

static ENTRY_COUNT: AtomicUsize = AtomicUsize::new(0);
static CURRENT_INDEX: AtomicUsize = AtomicUsize::new(0);
static LAST_POP_CANDIDATE_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_POP_CANDIDATE_STATE: AtomicUsize = AtomicUsize::new(0);
static LAST_POP_SELECTED_PTR: AtomicUsize = AtomicUsize::new(0);

struct LegacyTaskModel {
    tasks: [Option<usize>; MAX_TASKS],
}

impl LegacyTaskModel {
    const fn new() -> Self {
        Self {
            tasks: [None; MAX_TASKS],
        }
    }
}

static TASKS: Mutex<RawMutex, RefCell<LegacyTaskModel>> =
    Mutex::new(RefCell::new(LegacyTaskModel::new()));

fn enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_ESP_RADIO_TASK_MODEL_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || crate::esp_radio::backend_legacy_port_runtime_enabled()
}

pub(crate) fn runtime_mode_enabled() -> bool {
    enabled()
}

fn with_tasks<R>(f: impl FnOnce(&mut [Option<TaskPtr>; MAX_TASKS]) -> R) -> R {
    TASKS.lock(|shared| {
        let mut state = shared.borrow_mut();
        let mut tasks = [None; MAX_TASKS];
        for (idx, task) in state.tasks.iter().copied().enumerate() {
            tasks[idx] = task.and_then(|ptr| TaskPtr::new(ptr as *mut _));
        }
        let out = f(&mut tasks);
        for (idx, task) in tasks.iter().copied().enumerate() {
            state.tasks[idx] = task.map(|ptr| ptr.as_ptr() as usize);
        }
        out
    })
}

pub(crate) fn initialize_main_if_enabled() {
    if !enabled() {
        return;
    }

    with_tasks(|tasks| {
        if tasks[0].is_none() {
            tasks[0] = Some(SCHEDULER.current_task());
            ENTRY_COUNT.store(1, Ordering::Relaxed);
            CURRENT_INDEX.store(0, Ordering::Relaxed);
        }
    });
}

pub(crate) fn note_created_task(name: &str, task: TaskPtr) {
    if !enabled() {
        return;
    }

    initialize_main_if_enabled();
    with_tasks(|tasks| {
        let mut count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if tasks[..count]
            .iter()
            .flatten()
            .any(|existing| *existing == task)
        {
            return;
        }
        if count < MAX_TASKS {
            let insert_after = CURRENT_INDEX.load(Ordering::Relaxed).min(count);
            let insert_at = (insert_after + 1).min(count);
            let mut idx = count;
            while idx > insert_at {
                tasks[idx] = tasks[idx - 1];
                idx -= 1;
            }
            tasks[insert_at] = Some(task);
            count += 1;
            ENTRY_COUNT.store(count, Ordering::Relaxed);
        }
        if name == "wifi" {
            let wifi_index = tasks[..count]
                .iter()
                .position(|entry| *entry == Some(task))
                .unwrap_or(0);
            CURRENT_INDEX.store(wifi_index, Ordering::Relaxed);
        }
    });
}

pub(crate) fn note_deleted_task(task: Option<TaskPtr>) {
    if !enabled() {
        return;
    }

    let Some(task) = task else {
        return;
    };

    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        let mut kept = [None; MAX_TASKS];
        let mut next = 0;
        for entry in tasks.iter().take(count).copied().flatten() {
            if entry != task {
                kept[next] = Some(entry);
                next += 1;
            }
        }
        *tasks = kept;
        ENTRY_COUNT.store(next, Ordering::Relaxed);
        let current = CURRENT_INDEX.load(Ordering::Relaxed);
        CURRENT_INDEX.store(current.min(next.saturating_sub(1)), Ordering::Relaxed);
    });
}

pub(crate) fn current_task_override() -> Option<TaskPtr> {
    if !enabled() {
        return None;
    }
    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if count == 0 {
            return None;
        }
        tasks[CURRENT_INDEX.load(Ordering::Relaxed).min(count - 1)]
    })
}

pub(crate) fn yield_override() {
    if !enabled() {
        return;
    }
}

pub(crate) fn note_task_selected(task: TaskPtr) {
    if !enabled() {
        return;
    }

    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if let Some(idx) = tasks[..count].iter().position(|entry| *entry == Some(task)) {
            CURRENT_INDEX.store(idx, Ordering::Relaxed);
        }
    });
}

pub(crate) fn pop_next_ready_override(run_queue: &mut RunQueue) -> Option<TaskPtr> {
    if !enabled() {
        return None;
    }
    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if count == 0 {
            return None;
        }

        let start = CURRENT_INDEX.load(Ordering::Relaxed).min(count - 1);
        for step in 1..=count {
            let idx = (start + step) % count;
            let Some(task) = tasks[idx] else {
                continue;
            };
            let state = task.state();
            LAST_POP_CANDIDATE_PTR.store(task.as_ptr() as usize, Ordering::Relaxed);
            LAST_POP_CANDIDATE_STATE.store(state as usize, Ordering::Relaxed);
            if state != TaskState::Ready {
                continue;
            }
            run_queue.remove(task);
            CURRENT_INDEX.store(idx, Ordering::Relaxed);
            LAST_POP_SELECTED_PTR.store(task.as_ptr() as usize, Ordering::Relaxed);
            return Some(task);
        }

        LAST_POP_SELECTED_PTR.store(0, Ordering::Relaxed);
        None
    })
}

pub(crate) fn entry_count() -> usize {
    ENTRY_COUNT.load(Ordering::Relaxed)
}

pub(crate) fn current_index() -> usize {
    CURRENT_INDEX.load(Ordering::Relaxed)
}

pub(crate) fn ready_count() -> usize {
    if !enabled() {
        return 0;
    }

    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        tasks[..count]
            .iter()
            .flatten()
            .filter(|task| task.state() == TaskState::Ready)
            .count()
    })
}

pub(crate) fn reset() {
    ENTRY_COUNT.store(0, Ordering::Relaxed);
    CURRENT_INDEX.store(0, Ordering::Relaxed);
    LAST_POP_CANDIDATE_PTR.store(0, Ordering::Relaxed);
    LAST_POP_CANDIDATE_STATE.store(0, Ordering::Relaxed);
    LAST_POP_SELECTED_PTR.store(0, Ordering::Relaxed);
    with_tasks(|tasks| *tasks = [None; MAX_TASKS]);
}

pub(crate) fn task_ptr_at(index: usize) -> usize {
    if !enabled() {
        return 0;
    }
    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if index >= count {
            0
        } else {
            tasks[index].map(|task| task.as_ptr() as usize).unwrap_or(0)
        }
    })
}

pub(crate) fn task_state_at(index: usize) -> usize {
    if !enabled() {
        return 0;
    }
    with_tasks(|tasks| {
        let count = ENTRY_COUNT.load(Ordering::Relaxed).min(MAX_TASKS);
        if index >= count {
            0
        } else {
            tasks[index].map(|task| task.state() as usize).unwrap_or(0)
        }
    })
}

pub(crate) fn last_pop_candidate_ptr() -> usize {
    LAST_POP_CANDIDATE_PTR.load(Ordering::Relaxed)
}

pub(crate) fn last_pop_candidate_state() -> usize {
    LAST_POP_CANDIDATE_STATE.load(Ordering::Relaxed)
}

pub(crate) fn last_pop_selected_ptr() -> usize {
    LAST_POP_SELECTED_PTR.load(Ordering::Relaxed)
}
