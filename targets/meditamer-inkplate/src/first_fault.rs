//! First exception evidence, captured before xtensa-lx-rt saves a stack frame.
//!
//! The below-guard watchpoint previously led to a double exception inside
//! `save_context`, so the formatted trap frame contained the second fault's PC.
//! The runtime provides linker overrides for its user/kernel and debug
//! vectors. They record EPC1 or EPC6 before touching the stack, then transfer
//! to the unchanged default handlers. Normal level-1 interrupts are skipped.

use core::arch::global_asm;

const MAGIC: u32 = 0x4654_5531; // "FTU1"; format version 1

// Words 10..23 hold the interrupted A2..A15; word 24 is temporary storage
// for A2 while the naked handler records the special registers. The record
// is committed by writing MAGIC last.
#[used]
#[no_mangle]
#[link_section = ".rtc_slow.persistent"]
static mut __meditamer_first_fault: [u32; 25] = [0; 25];

global_asm!(
    r#"
    .section .rwtext,"ax",@progbits
    .literal .Lfirst_fault_ptr, __meditamer_first_fault
    .literal .Lfirst_fault_magic, 0x46545531
    .literal .Lfirst_fault_uart0, 0x3ff40000

    // UART0 FIFO is 128 bytes. Emit at most 60 bytes only when at least 64
    // are free, so the first fault survives even if the next handler faults
    // before the retained record can be read at boot. No stack or locks.
    .macro first_fault_hex offset
        l32i a2, a6, \offset
        movi a5, 8
.Lfirst_fault_hex_loop_\@:
        srli a4, a2, 28
        bltui a4, 10, .Lfirst_fault_digit_\@
        addi a4, a4, 87
        j .Lfirst_fault_hex_send_\@
.Lfirst_fault_digit_\@:
        addi a4, a4, 48
.Lfirst_fault_hex_send_\@:
        s32i a4, a3, 0
        slli a2, a2, 4
        addi a5, a5, -1
        bnez a5, .Lfirst_fault_hex_loop_\@
    .endm

.Lfirst_fault_uart_emit:
    l32r a3, .Lfirst_fault_uart0
    l32i a4, a3, 28
    extui a4, a4, 16, 8
    movi a5, 64
    blt a4, a5, .Lfirst_fault_uart_ready
    j .Lfirst_fault_uart_done
.Lfirst_fault_uart_ready:
    movi a4, 10
    s32i a4, a3, 0
    movi a4, 33
    s32i a4, a3, 0
    movi a4, 70
    s32i a4, a3, 0
    s32i a7, a3, 0
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 4
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 12
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 16
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 24
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 56
    movi a4, 32
    s32i a4, a3, 0
    first_fault_hex 60
    movi a4, 10
    s32i a4, a3, 0
.Lfirst_fault_uart_done:
    ret

    .global __naked_user_exception
    .type __naked_user_exception,@function
    .p2align 2
__naked_user_exception:
    rsr a0, EXCCAUSE
    beqi a0, 4, .Lfirst_fault_user_default
    call0 .Lfirst_fault_level_1
.Lfirst_fault_user_default:
    call0 __default_naked_exception

    .global __naked_kernel_exception
    .type __naked_kernel_exception,@function
    .p2align 2
__naked_kernel_exception:
    rsr a0, EXCCAUSE
    beqi a0, 4, .Lfirst_fault_kernel_default
    call0 .Lfirst_fault_level_1
.Lfirst_fault_kernel_default:
    call0 __default_naked_exception

.Lfirst_fault_level_1:
    l32r a0, .Lfirst_fault_ptr
    s32i a2, a0, 96
    l32i a2, a0, 0
    bnez a2, .Lfirst_fault_level_1_existing
    l32i a2, a0, 96
    s32i a2, a0, 40
    s32i a3, a0, 44
    s32i a4, a0, 48
    s32i a5, a0, 52
    s32i a6, a0, 56
    s32i a7, a0, 60
    s32i a8, a0, 64
    s32i a9, a0, 68
    s32i a10, a0, 72
    s32i a11, a0, 76
    s32i a12, a0, 80
    s32i a13, a0, 84
    s32i a14, a0, 88
    s32i a15, a0, 92
    rsr a2, EPC1
    s32i a2, a0, 4
    rsr a2, PS
    s32i a2, a0, 8
    s32i a1, a0, 12
    movi a2, 0
    s32i a2, a0, 16
    rsr a2, EXCCAUSE
    s32i a2, a0, 20
    rsr a2, EXCVADDR
    s32i a2, a0, 24
    rsr a2, DEPC
    s32i a2, a0, 28
    rsr a2, EXCSAVE1
    s32i a2, a0, 32
    rsr a2, PRID
    s32i a2, a0, 36
    l32r a2, .Lfirst_fault_magic
    s32i a2, a0, 0
    movi a7, 49
    mov a6, a0
    call0 .Lfirst_fault_uart_emit
.Lfirst_fault_level_1_done:
    l32r a0, .Lfirst_fault_ptr
    l32i a2, a0, 96
    l32i a3, a0, 44
    l32i a4, a0, 48
    l32i a5, a0, 52
    l32i a6, a0, 56
    l32i a7, a0, 60
    call0 __default_naked_exception
.Lfirst_fault_level_1_existing:
    l32i a2, a0, 96
    call0 __default_naked_exception

    .global __naked_level_6_interrupt
    .type __naked_level_6_interrupt,@function
    .p2align 2
__naked_level_6_interrupt:
    // EXCSAVE6 already holds the interrupted A0. Save all registers before
    // borrowing A2-A7 for the stack-free UART trace.
    l32r a0, .Lfirst_fault_ptr
    s32i a2, a0, 96
    l32i a2, a0, 0
    bnez a2, .Lfirst_fault_existing
    l32i a2, a0, 96
    s32i a2, a0, 40
    s32i a3, a0, 44
    s32i a4, a0, 48
    s32i a5, a0, 52
    s32i a6, a0, 56
    s32i a7, a0, 60
    s32i a8, a0, 64
    s32i a9, a0, 68
    s32i a10, a0, 72
    s32i a11, a0, 76
    s32i a12, a0, 80
    s32i a13, a0, 84
    s32i a14, a0, 88
    s32i a15, a0, 92
    rsr a2, EPC6
    s32i a2, a0, 4
    rsr a2, EPS6
    s32i a2, a0, 8
    s32i a1, a0, 12
    rsr a2, DEBUGCAUSE
    s32i a2, a0, 16
    rsr a2, EXCCAUSE
    s32i a2, a0, 20
    rsr a2, EXCVADDR
    s32i a2, a0, 24
    rsr a2, DEPC
    s32i a2, a0, 28
    rsr a2, EXCSAVE6
    s32i a2, a0, 32
    rsr a2, PRID
    s32i a2, a0, 36
    l32r a2, .Lfirst_fault_magic
    s32i a2, a0, 0
    movi a7, 54
    mov a6, a0
    call0 .Lfirst_fault_uart_emit
.Lfirst_fault_done:
    l32r a0, .Lfirst_fault_ptr
    l32i a2, a0, 96
    l32i a3, a0, 44
    l32i a4, a0, 48
    l32i a5, a0, 52
    l32i a6, a0, 56
    l32i a7, a0, 60
    call0 __default_naked_level_6_interrupt
.Lfirst_fault_existing:
    l32i a2, a0, 96
    call0 __default_naked_level_6_interrupt
    "#
);

/// Read and clear the one-shot trap record at startup, before Wi-Fi starts.
pub(crate) fn take() -> Option<[u32; 24]> {
    let ptr = core::ptr::addr_of_mut!(__meditamer_first_fault) as *mut u32;
    // SAFETY: this runs during single-core boot before debug watchpoints arm.
    // The section is NOLOAD, and all reads/writes are volatile because the
    // vector handler accesses it outside Rust's ordinary control flow.
    unsafe {
        let mut words = [0u32; 24];
        for (index, word) in words.iter_mut().enumerate() {
            *word = ptr.add(index).read_volatile();
        }
        ptr.write_volatile(0);
        (words[0] == MAGIC).then_some(words)
    }
}
