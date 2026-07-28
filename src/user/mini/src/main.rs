//! Mini test: 仅输出一个字符验证 syscall 通路
#![no_std]
#![no_main]

use userlib::*;
use userlib::sys::*;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print_char(b'!');   // 仅输出 '!'
    loop { core::hint::spin_loop(); }
}
