//! ESP32 only: scheduler transitions plus the three HAL-dispatched interrupt
//! levels. Link the final binary with --wrap=__level_{1,2,3}_interrupt.
//! Trace entrypoints are explicitly in IRAM: the upstream global_trace!
//! macro cannot place its exported forwarding functions there.
use super::{Core, Event, Reading};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};
use esp_hal::{peripherals::Interrupt, ram, system::Cpu, time::Instant, trapframe::TrapFrame};

// An esp-rtos task pointer is either null (idle) or in internal DRAM. Capture
// the first impossible saved thread pointer on each core at the level-1/2/3
// interrupt boundary. The pre/post distinction identifies whether the
// interrupted context was already bad or dispatch selected a bad context.
// This RTC-slow evidence survives the timer-group watchdog reset and consumes
// no linked DRAM. One core owns each seven-word slot; MAGIC is committed last.
const TASK_POINTER_TRACE_MAGIC: u32 = 0x5450_5232; // TPR2
#[link_section = ".rtc_slow.persistent"]
static mut TASK_POINTER_TRACE: [[u32; 7]; 2] = [[0; 7]; 2];

// Each core records the most recently dispatched IRQ and whether its handler
// returned. This never dereferences an interrupt frame. A later IRQ can
// overwrite it, so it is context for a bad outer frame, not writer proof.
// Encoded as ((id + 1) << 2) | 1 (entered) or | 2 (returned).
#[link_section = ".rtc_slow.persistent"]
static mut LAST_IRQ_MARKER: [u32; 2] = [0; 2];
static TOUCH_TIMER_IRQ_AT_US: AtomicU32 = AtomicU32::new(0);
static TOUCH_TIMER_IRQ_COUNT: AtomicU32 = AtomicU32::new(0);

#[inline(always)]
fn plausible_task_pointer(ptr: u32) -> bool {
    ptr == 0 || (0x3ff0_0000..0x4000_0000).contains(&ptr)
}

#[inline(always)]
unsafe fn trace_task_pointer(frame: *mut (), stage: u32, before: u32, after: u32) {
    if plausible_task_pointer(after) {
        return;
    }
    let core = Cpu::current() as usize;
    let slot = unsafe {
        core::ptr::addr_of!(TASK_POINTER_TRACE)
            .cast::<u32>()
            .add(core * 7)
    };
    if unsafe { slot.read_volatile() } == TASK_POINTER_TRACE_MAGIC {
        return;
    }
    let context = frame.cast::<TrapFrame>();
    let pc = unsafe { core::ptr::addr_of!((*context).PC).read_volatile() };
    let sp = unsafe { core::ptr::addr_of!((*context).A1).read_volatile() };
    record_bad_task_pointer(stage, before, after, pc, sp);
}

#[inline(always)]
fn record_bad_task_pointer(stage: u32, before: u32, after: u32, pc: u32, sp: u32) {
    let core = Cpu::current() as usize;
    let slot = unsafe {
        core::ptr::addr_of_mut!(TASK_POINTER_TRACE)
            .cast::<u32>()
            .add(core * 7)
    };
    // SAFETY: each core only writes its own slot; boot reads before either
    // executor starts. An interrupted incomplete write has no commit marker.
    unsafe {
        if slot.read_volatile() == TASK_POINTER_TRACE_MAGIC {
            return;
        }
        slot.add(1).write_volatile(stage);
        slot.add(2).write_volatile(before);
        slot.add(3).write_volatile(after);
        slot.add(4).write_volatile(pc);
        slot.add(5).write_volatile(sp);
        slot.add(6).write_volatile(
            core::ptr::addr_of!(LAST_IRQ_MARKER)
                .cast::<u32>()
                .add(core)
                .read_volatile(),
        );
        slot.write_volatile(TASK_POINTER_TRACE_MAGIC);
    }
}

// Called only after the HAL itself sampled an impossible frame THREADPTR.
// All arguments are values: this callback must never dereference the inner
// handler's frame, which made an earlier diagnostic fault recursively.
// stage: bit 16 marks an inner boundary, bits 8..14 hold the IRQ ID, and
// bits 0..1 mean entry / profiler begin / handler return / profiler end.
#[no_mangle]
#[ram]
fn cpu_profile_bad_task_pointer(id: u32, phase: u32, before: u32, after: u32, pc: u32, sp: u32) {
    record_bad_task_pointer(0x1_0000 | (id << 8) | phase, before, after, pc, sp);
}

/// Read and clear the first bad task-pointer transition on each core at boot.
pub fn take_task_pointer_trace() -> [Option<[u32; 6]>; 2] {
    let mut result = [None; 2];
    for (core, item) in result.iter_mut().enumerate() {
        let slot = unsafe {
            core::ptr::addr_of_mut!(TASK_POINTER_TRACE)
                .cast::<u32>()
                .add(core * 7)
        };
        // SAFETY: single-core boot reads before IRQ wrappers can write slots.
        unsafe {
            if slot.read_volatile() == TASK_POINTER_TRACE_MAGIC {
                let mut words = [0; 6];
                for (index, word) in words.iter_mut().enumerate() {
                    *word = slot.add(index + 1).read_volatile();
                }
                *item = Some(words);
            }
            slot.write_volatile(0);
            core::ptr::addr_of_mut!(LAST_IRQ_MARKER)
                .cast::<u32>()
                .add(core)
                .write_volatile(0);
        }
    }
    result
}

#[inline(always)]
fn mark_irq(id: u32, stage: u32) {
    let core = Cpu::current() as usize;
    // SAFETY: the calling core owns this slot; there is no shared mutation.
    unsafe {
        core::ptr::addr_of_mut!(LAST_IRQ_MARKER)
            .cast::<u32>()
            .add(core)
            .write_volatile(((id + 1) << 2) | stage);
    }
}

struct State {
    cores: [Core; 2],
    profiles: [crate::profile::Profile; 2],
}
struct Storage(UnsafeCell<State>);
// SAFETY: every access goes through with_cores, which masks local interrupts
// before taking the cross-core lock. Its closures never yield or take another
// lock. This tiny path must remain inlined/IRAM even with flash cache disabled.
unsafe impl Sync for Storage {}
static CORES: Storage = Storage(UnsafeCell::new(State {
    cores: [Core::new(); 2],
    profiles: [crate::profile::Profile::new(); 2],
}));
static LOCK: AtomicBool = AtomicBool::new(false);
/// A flash write can hardware-stall the other CPU at any instruction. Do not
/// let it stall that CPU while a profiling hook owns the cross-core lock.
static FLASH_PAUSED: AtomicBool = AtomicBool::new(false);
static ACTIVE_HOOKS: AtomicU32 = AtomicU32::new(0);
/// Holding core while `LOCK` is set: 0 = none, 1 = ProCpu, 2 = AppCpu.
/// Written only by the lock holder and read by wedged spinners, so the
/// wedge panic can name its holder: holder == spinner core means a
/// same-core re-entrant acquisition through an above-mask preempter;
/// holder == other core means that core stalled while holding; holder == 0
/// with the lock set means the lock word itself was clobbered in memory.
static HOLDER: AtomicU32 = AtomicU32::new(0);

/// Spins before declaring the profile lock wedged. Normal holds are a few
/// field updates (~100 ns); cross-core contention resolves in the same
/// order. Anything approaching this bound is a wedged holder, and a silent
/// permanent spin here freezes both cores' executors -- panic loudly
/// (the crumb hook runs first) instead.
const LOCK_WEDGE_SPINS: u32 = 1 << 20;

#[inline(always)]
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    xtensa_lx::interrupt::free(|| {
        // Register before checking the gate: a flash writer that observes zero
        // active hooks can then safely park the other CPU. SeqCst orders the
        // registration, gate check, and quiescence observation across cores.
        ACTIVE_HOOKS.fetch_add(1, Ordering::SeqCst);
        if FLASH_PAUSED.load(Ordering::SeqCst) {
            ACTIVE_HOOKS.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let me = Cpu::current() as u32 + 1;
        let mut spins = 0u32;
        loop {
            match LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => break,
                Err(_) => {
                    spins = spins.wrapping_add(1);
                    if spins >= LOCK_WEDGE_SPINS {
                        panic!(
                            "cpu-load profile lock wedged holder={} me={}",
                            HOLDER.load(Ordering::Acquire),
                            me,
                        );
                    }
                    core::hint::spin_loop();
                }
            }
        }
        HOLDER.store(me, Ordering::Release);
        // SAFETY: local interrupts are masked and the exclusive cross-core
        // lock is held until the non-panicking closure returns.
        let result = f(unsafe { &mut *CORES.0.get() });
        HOLDER.store(0, Ordering::Relaxed);
        LOCK.store(false, Ordering::Release);
        ACTIVE_HOOKS.fetch_sub(1, Ordering::SeqCst);
        Some(result)
    })
}

/// Keep profiler hooks out of the cross-core lock while flash may park a CPU.
/// Dropping the guard starts a fresh window because an interrupt can enter
/// before the pause and exit during it, leaving paired accounting incomplete.
pub struct FlashPauseGuard;

pub fn pause_for_flash() -> FlashPauseGuard {
    assert!(
        FLASH_PAUSED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok(),
        "cpu-load flash pause entered twice"
    );
    let mut spins = 0u32;
    while ACTIVE_HOOKS.load(Ordering::SeqCst) != 0 {
        spins = spins.wrapping_add(1);
        assert!(
            spins < LOCK_WEDGE_SPINS,
            "cpu-load flash pause did not quiesce"
        );
        core::hint::spin_loop();
    }
    FlashPauseGuard
}

impl Drop for FlashPauseGuard {
    fn drop(&mut self) {
        // With the gate closed, no hook can access CORES even after the other
        // CPU has been unparked. The flash driver returns before this drop.
        let state = unsafe { &mut *CORES.0.get() };
        state.cores = [Core::new(); 2];
        state.profiles = [crate::profile::Profile::new(); 2];
        FLASH_PAUSED.store(false, Ordering::SeqCst);
    }
}

#[inline(always)]
fn now_us() -> u32 {
    Instant::now().duration_since_epoch().as_micros() as u32
}

/// Both per-core slots for the calling core without a bounds-check panic.
///
/// Takes the `Cpu` enum itself so an out-of-range index is unrepresentable.
/// Two-element destructuring plus exhaustive matching keeps this panic-free
/// (no `panic_bounds_check` for the IRAM gate to count) with no `unsafe`:
/// `Cpu::current()` can only yield `ProCpu`/`AppCpu` (esp-hal 1.2.0
/// `system.rs`; any other raw id is `unreachable!` inside the HAL).
#[inline(always)]
fn slots(state: &mut State, core: Cpu) -> (&mut Core, &mut crate::profile::Profile) {
    let [core0, core1] = &mut state.cores;
    let [profile0, profile1] = &mut state.profiles;
    match core {
        Cpu::ProCpu => (core0, profile0),
        Cpu::AppCpu => (core1, profile1),
    }
}

#[ram]
fn event<const KIND: u8>() {
    let _ = with_state(|state| {
        // Constant specialization avoids a flash-resident jump table in this
        // interrupt path when the firmware places switch tables in flash.
        let event = match KIND {
            0 => Event::Task,
            1 => Event::Idle,
            2 => Event::InterruptEnter,
            _ => Event::InterruptExit,
        };
        let now = now_us();
        let (core_slot, profile) = slots(state, Cpu::current());
        core_slot.event(now, event);
        match KIND {
            1 => profile.thread(now, 0),
            2 => profile.irq_begin(now, 128),
            3 => profile.irq_end(now),
            _ => {}
        }
    });
}

/// Single consumer: sample once per reporting window, then share the result.
pub fn sample() -> [Option<Reading>; 2] {
    with_state(|state| {
        let now = now_us();
        [state.cores[0].sample(now), state.cores[1].sample(now)]
    })
    .unwrap_or([None; 2])
}

#[no_mangle]
#[ram]
fn _rtos_trace_task_exec_begin(id: u32) {
    event::<0>();
    let _ = with_state(|state| slots(state, Cpu::current()).1.thread(now_us(), id));
}
#[no_mangle]
#[ram]
fn _rtos_trace_system_idle() {
    event::<1>();
}

macro_rules! noop {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        #[no_mangle]
        #[ram]
        fn $name($($arg: $ty),*) {}
    };
}
noop!(_rtos_trace_start());
noop!(_rtos_trace_stop());
noop!(_rtos_trace_task_new(_id: u32));
noop!(_rtos_trace_task_send_info(_id: u32, _info: rtos_trace::TaskInfo));
noop!(_rtos_trace_task_new_stackless(_id: u32, _name: &'static str, _priority: u32));
noop!(_rtos_trace_task_terminate(_id: u32));
noop!(_rtos_trace_task_exec_end());
noop!(_rtos_trace_task_ready_begin(_id: u32));
noop!(_rtos_trace_task_ready_end(_id: u32));
noop!(_rtos_trace_isr_enter());
noop!(_rtos_trace_isr_exit());
noop!(_rtos_trace_isr_exit_to_scheduler());
noop!(_rtos_trace_name_marker(_id: u32, _name: &'static str));
noop!(_rtos_trace_marker(_id: u32));
noop!(_rtos_trace_marker_begin(_id: u32));
noop!(_rtos_trace_marker_end(_id: u32));

macro_rules! wrap_interrupt {
    ($wrapper:ident, $original:ident) => {
        extern "Rust" {
            fn $original(frame: *mut ());
        }
        #[no_mangle]
        #[ram]
        #[allow(non_snake_case)] // GNU ld --wrap symbol spelling is fixed.
        unsafe fn $wrapper(frame: *mut ()) {
            // The vector saved THREADPTR in this trap frame. On return, the
            // RTOS may have replaced it with the next task's context.
            let before = unsafe {
                core::ptr::addr_of!((*frame.cast::<TrapFrame>()).THREADPTR).read_volatile()
            };
            unsafe { trace_task_pointer(frame, 1, before, before) };
            event::<2>();
            // SAFETY: the Xtensa vector passes its original Context pointer
            // unchanged. The linker resolves __real_ to the original HAL
            // handler, whose Rust ABI receives that same single pointer.
            unsafe {
                $original(frame);
            }
            // Sample the same outer vector frame before the profiler exit
            // hook. This distinguishes corruption inside dispatch from a
            // write by event::<3>() without passing the frame to an inner
            // handler callback (which previously faulted in the probe).
            let after_dispatch = unsafe {
                core::ptr::addr_of!((*frame.cast::<TrapFrame>()).THREADPTR).read_volatile()
            };
            unsafe { trace_task_pointer(frame, 2, before, after_dispatch) };
            event::<3>();
            let after_profile_exit = unsafe {
                core::ptr::addr_of!((*frame.cast::<TrapFrame>()).THREADPTR).read_volatile()
            };
            unsafe { trace_task_pointer(frame, 3, after_dispatch, after_profile_exit) };
        }
    };
}
wrap_interrupt!(__wrap___level_1_interrupt, __real___level_1_interrupt);
wrap_interrupt!(__wrap___level_2_interrupt, __real___level_2_interrupt);
wrap_interrupt!(__wrap___level_3_interrupt, __real___level_3_interrupt);

/// Copies a consistent report into caller-owned storage. The caller runs with
/// caches available; IRQ hooks access only the two internal live profiles.
pub fn profile_snapshot(core: usize, destination: &mut crate::profile::Snapshot) {
    if with_state(|state| {
        state.profiles[core].account(now_us());
        destination.capture(&state.profiles[core]);
    })
    .is_none()
    {
        *destination = crate::profile::Snapshot::new();
        destination.timings.2 = 1;
    }
}
#[no_mangle]
#[ram]
fn cpu_profile_irq_begin(id: u32) {
    mark_irq(id, 1);
    let core = Cpu::current();
    let _ = with_state(|s| {
        let now = now_us();
        if core == Cpu::AppCpu && id == Interrupt::TG1_T0_LEVEL as u32 {
            TOUCH_TIMER_IRQ_AT_US.store(now, Ordering::Relaxed);
            TOUCH_TIMER_IRQ_COUNT.fetch_add(1, Ordering::Release);
        }
        slots(s, core).1.irq_begin(now, id as usize)
    });
}

pub fn touch_timer_irq_snapshot() -> (u32, u32) {
    let count = TOUCH_TIMER_IRQ_COUNT.load(Ordering::Acquire);
    let at_us = TOUCH_TIMER_IRQ_AT_US.load(Ordering::Relaxed);
    (count, at_us)
}
#[no_mangle]
#[ram]
fn cpu_profile_irq_end(id: u32) {
    mark_irq(id, 2);
    let _ = with_state(|s| slots(s, Cpu::current()).1.irq_end(now_us()));
}
#[no_mangle]
fn _embassy_trace_task_exec_begin(_executor: u32, task: u32) {
    let _ = with_state(|s| s.profiles[Cpu::current() as usize].begin(now_us(), task));
}
#[no_mangle]
fn _embassy_trace_task_exec_end(_executor: u32, _task: u32) {
    let _ = with_state(|s| s.profiles[Cpu::current() as usize].end(now_us()));
}
noop!(_embassy_trace_poll_start(_executor: u32));
noop!(_embassy_trace_task_new(_executor: u32, _task: u32));
noop!(_embassy_trace_task_end(_executor: u32, _task: u32));
#[no_mangle]
#[ram]
fn _embassy_trace_task_ready_begin(_executor: u32, task: u32) {
    let _ = with_state(|s| {
        let now = now_us();
        // Destructure rather than index so this IRAM path keeps no
        // `panic_bounds_check` for constant slots either.
        let [profile0, profile1] = &mut s.profiles;
        if !profile0.ready(now, task) {
            profile1.ready(now, task);
        }
    });
}
noop!(_embassy_trace_executor_idle(_executor: u32));

/// Start a new latency window without clearing cumulative CPU/IRQ accounting.
pub fn reset_profile_timings() {
    let _ = with_state(|state| {
        let now = now_us();
        for profile in &mut state.profiles {
            profile.reset_timings(now);
        }
    });
}

pub fn tail_begin() {
    let _ = with_state(|state| state.profiles[1].start_tail(now_us()));
}

pub fn tail_end() -> Option<crate::profile::TailWork> {
    with_state(|state| state.profiles[1].stop_tail(now_us())).flatten()
}

pub fn tail_note_pipeline(branch: crate::profile::PipelineBranch) {
    let _ = with_state(|state| state.profiles[1].note_tail_pipeline(branch));
}

pub fn tail_note_pipeline_phase(phase: crate::profile::PipelinePhase) {
    let _ = with_state(|state| state.profiles[1].note_tail_pipeline_phase(now_us(), phase));
}

pub fn tail_note_touch_phase(phase: crate::profile::TouchPhase) {
    let _ = with_state(|state| state.profiles[1].note_tail_touch_phase(now_us(), phase));
}

/// Cache-on application tracing only. Never call from inside the profiling hooks.
pub fn execution_context() -> (u8, u8, u32) {
    with_state(|state| {
        let core = Cpu::current() as usize;
        let (kind, id) = state.profiles[core].execution_context();
        (core as u8, kind, id)
    })
    .unwrap_or((Cpu::current() as u8, 0, 0))
}
