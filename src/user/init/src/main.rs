//! QueenX Init — fork 测试 (print_char)

#![no_std]
#![no_main]

use userlib::*;
use userlib::sys::*;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { proc_exit(1); }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print_char(b'X');
    print_char(b'\n');
    let _c = fork();
    print_char(b'Y');
    print_char(b'\n');
    loop { proc_yield(); }
}
