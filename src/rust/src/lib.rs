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

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
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
