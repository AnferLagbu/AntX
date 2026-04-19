#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(asm)]
#![feature(str_as_str)]

extern crate alloc;

mod memory_allocator;
pub mod proc;
pub mod fs;

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
pub extern "C" fn rust_kernel_init() {
    crate::proc::scheduler::init();
    crate::fs::vfs::init();
}
