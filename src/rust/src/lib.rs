#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm)]
#![feature(str_as_str)]

extern crate alloc;

mod memory_allocator;

// Memory Management Subsystem (Rust rewrite of kernel/mm)
#[path = "../../mm/mod.rs"]
pub mod mm;

#[path = "../../fs/mod.rs"]
pub mod fs;

#[path = "../../proc/mod.rs"]
pub mod proc;

#[path = "../../pwid/mod.rs"]
pub mod pwid;

#[path = "../../dma/mod.rs"]
pub mod dma;

use core::panic::PanicInfo;
use core::sync::atomic::Ordering;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Signal to the scheduler/IDT that a recoverable panic occurred
    crate::proc::recovery::PANIC_FLAG.store(true, Ordering::SeqCst);

    // Store panic message for recovery diagnostics
    let msg = alloc::format!("{}", info);
    let bytes = msg.as_bytes();
    let len = bytes.len().min(127);
    unsafe {
        crate::proc::recovery::PANIC_MSG[..len].copy_from_slice(&bytes[..len]);
        crate::proc::recovery::PANIC_MSG[len] = 0;
    }

    // Trigger int 0x82 — dedicated recovery interrupt.
    // The IDT handler will check PANIC_FLAG → attempt domain recovery → return.
    // If recovery fails, it falls through to kernel panic.
    unsafe {
        core::arch::asm!("int 0x82", options(noreturn));
    }
}

#[alloc_error_handler]
fn alloc_error(layout: alloc::alloc::Layout) -> ! {
    panic!("Allocation error: {:?}", layout);
}

#[no_mangle]
pub extern "C" fn kernel_init() {
    crate::proc::scheduler::init();
    crate::fs::vfs::init();
}
