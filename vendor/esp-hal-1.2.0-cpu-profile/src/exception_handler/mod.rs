use crate::trapframe::TrapFrame;

#[cfg(xtensa)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".rwtext")]
unsafe extern "C" fn __user_exception(
    cause: xtensa_lx_rt::exception::ExceptionCause,
    context: &TrapFrame,
) {
    panic!(
        "\n\nException occurred on {:?} '{:?}'\n{:?}",
        crate::system::Cpu::current(),
        cause,
        context
    );
}

#[cfg(all(xtensa, stack_guard_monitoring))]
#[inline(always)]
pub(crate) fn breakpoint_interrupt(context: &TrapFrame) {
    let mut dbgcause: u32;
    unsafe {
        core::arch::asm!(
            "rsr.debugcause {0}",
            out(reg) dbgcause, options(nostack)
        );
    }

    if dbgcause & 4 != 0 {
        // Xtensa raises DBREAK *instead of* executing the matching access.
        // The watched words can therefore be intact at a genuine hit. DBNUM
        // (DEBUGCAUSE bits 8..11), not post-store damage, identifies the
        // channel. Record the live watchpoint registers too, because their
        // configured addresses could themselves have been corrupted.
        let channel = (dbgcause >> 8) & 0xf;
        let (mut dbreaka0, mut dbreakc0, mut dbreaka1, mut dbreakc1): (u32, u32, u32, u32);
        unsafe {
            core::arch::asm!(
                "rsr.dbreaka0 {a0}",
                "rsr.dbreakc0 {c0}",
                "rsr.dbreaka1 {a1}",
                "rsr.dbreakc1 {c1}",
                a0 = out(reg) dbreaka0,
                c0 = out(reg) dbreakc0,
                a1 = out(reg) dbreaka1,
                c1 = out(reg) dbreakc1,
                options(nostack)
            );
        }
        let (guard_damaged, sentinel_damaged) = unsafe {
            unsafe extern "C" {
                static mut __stack_chk_guard: u32;
            }
            let guard = core::ptr::addr_of_mut!(__stack_chk_guard) as *mut u32;
            let guard_value: u32 =
                esp_config::esp_config_int!(u32, "ESP_HAL_CONFIG_STACK_GUARD_VALUE");
            (
                guard.read_volatile() != guard_value,
                guard.byte_offset(-4).read_volatile() != crate::debugger::below_guard_canary(),
            )
        };
        panic!(
            "\n\nDBREAK on {:?} channel={} debugcause=0x{:08x} guard_changed={} sentinel_changed={} dbreaka0=0x{:08x} dbreakc0=0x{:08x} dbreaka1=0x{:08x} dbreakc1=0x{:08x}\n{:?}",
            crate::system::Cpu::current(),
            channel,
            dbgcause,
            guard_damaged,
            sentinel_damaged,
            dbreaka0,
            dbreakc0,
            dbreaka1,
            dbreakc1,
            context
        );
    }

    panic!(
        "\n\nBreakpoint on {:?}\n{:?}\nDebug cause: {}",
        crate::system::Cpu::current(),
        context,
        dbgcause
    );
}

#[cfg(riscv)]
#[unsafe(no_mangle)]
unsafe extern "C" fn ExceptionHandler(context: &TrapFrame) -> ! {
    let mepc = riscv::register::mepc::read();
    let code = riscv::register::mcause::read().bits() & property!("soc.cpu_mcause_mask");
    let mtval = riscv::register::mtval::read();

    unsafe extern "C" {
        static mut __stack_chk_guard: u32;
    }

    let core = crate::system::Cpu::current();

    if code == 14 {
        panic!(
            "[{:?}] Stack overflow detected at 0x{:x}, possibly called by 0x{:x}",
            core, mepc, context.ra
        );
    }

    #[cfg(stack_guard_monitoring)]
    if code == 3 {
        let guard_addr = core::ptr::addr_of_mut!(__stack_chk_guard) as *mut _ as usize;

        if mtval == guard_addr {
            panic!(
                "[{:?}] Detected a write to the main stack's guard value at 0x{:x}, possibly called by 0x{:x}",
                core, mepc, context.ra
            )
        } else {
            if unsafe { crate::debugger::watchpoint_hit(1) } {
                panic!(
                    "[{:?}] Detected a write to the trap/rwtext segment at 0x{:x}, possibly called by 0x{:x}",
                    core, mepc, context.ra
                );
            } else if unsafe { crate::debugger::watchpoint_hit(0) } {
                panic!(
                    "[{:?}] Detected a write to a stack guard value at 0x{:x}, possibly called by 0x{:x}",
                    core, mepc, context.ra
                );
            } else {
                panic!(
                    "[{:?}] Breakpoint exception at 0x{:08x}, mtval=0x{:08x}\n{:?}",
                    core, mepc, mtval, context
                );
            }
        }
    }

    let code_str = match code {
        0 => "Instruction address misaligned",
        1 => "Instruction access fault",
        2 => "Illegal instruction",
        3 => "Breakpoint",
        4 => "Load address misaligned",
        5 => "Load access fault",
        6 => "Store/AMO address misaligned",
        7 => "Store/AMO access fault",
        8 => "Environment call from U-mode",
        9 => "Environment call from S-mode",
        10 => "Reserved",
        11 => "Environment call from M-mode",
        12 => "Instruction page fault",
        13 => "Load page fault",
        14 => "Reserved",
        15 => "Store/AMO page fault",
        #[cfg(esp32p4)]
        0x1f => "Illegal PIE instruction",
        _ => "UNKNOWN",
    };

    panic!(
        "[{:?}] Exception '{} ({:x})' mepc=0x{:08x}, mtval=0x{:08x}\n{:?}",
        core, code_str, code, mepc, mtval, context
    );
}
