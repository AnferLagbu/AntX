//! AntX 独立安装程序
//!
//! 可作为独立二进制运行，执行完整 6 步系统安装。

#![no_std]
#![no_main]

use userlib::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print("[install] PANIC: ");
    if let Some(loc) = info.location() {
        print("at "); print(loc.file());
        print(":"); print_dec(loc.line() as i64);
    }
    print("\n");
    proc_exit(1);
}

fn install_main() {
    install_wizard::run();
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    install_main();
    proc_exit(0);
}
