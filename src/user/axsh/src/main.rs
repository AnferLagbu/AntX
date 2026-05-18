//! axsh — AntX Shell
//!
//! 模块化 Shell: 主循环 + 提示符在此，命令实现位于 `commands/` 子目录。

#![no_std]
#![no_main]

mod commands;

use userlib::*;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    userlib::print("[axsh] PANIC: ");
    if let Some(loc) = info.location() {
        userlib::print("at "); userlib::print(loc.file());
        userlib::print(":"); print_dec(loc.line() as i64);
    }
    userlib::print("\n");
    proc_exit(1);
}

fn banner() {
    println(""); println("  ___  _  _ ___");
    println(" / _ \\| || | __|"); println("| (_) | || |__ \\");
    println(" \\___/|_||_|___/"); println("");
    println("axsh - AntX Shell"); println("Type 'help' for commands"); println("");
}

fn prompt() {
    let mut cwd = [0u8; 64]; let _ = env_getcwd(&mut cwd);
    let cwd_str = core::str::from_utf8(&cwd).unwrap_or("/").trim_end_matches('\0');
    let pwid = proc_get_pwid();
    if pwid != 0 { print("["); print_hex(pwid); print("]"); }
    print(if cwd_str.is_empty() { "/" } else { cwd_str }); print("> ");
}

fn shell_main() {
    banner();
    let mut line = [0u8; 256];
    while commands::is_running() {
        prompt();
        let len = read_line(&mut line); if len == 0 { continue; }
        let (args, argc) = parse_args(&line[..len]); if argc == 0 { continue; }
        commands::dispatch(args, argc);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! { shell_main(); proc_exit(0); }
