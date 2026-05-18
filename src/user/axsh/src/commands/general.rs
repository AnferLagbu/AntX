/// 通用命令: help, cls, echo, exit

use userlib::{print, println};

use super::{Cmd, as_str};

pub fn help(_: &Cmd) {
    println(""); println("axsh - AntX Shell Commands"); println("=========================="); println("");
    println("General:"); println("  help          Show this help");
    println("  cls           Clear screen"); println("  echo [text]   Print text");
    println("  exit          Exit shell"); println("");
    println("File (f*):"); println("  fls [path]    List directory");
    println("  fcd <dir>     Change directory"); println("  fpwd          Print working directory");
    println("  fcat <file>   Display file"); println("  fmk <file>    Create file");
    println("  fmd <dir>     Create directory"); println("  frm <path>    Remove file/dir");
    println("  fput <f> <t>  Write text to file"); println("  fsync         Sync to disk");
    println(""); println("Identity (i*):");
    println("  ilogin <n> <pw>  Login with note and password");
    println("  ilogout         Logout"); println("  iwho            Show current PWID");
    println("  ipasswd         Change password"); println("");
    println("System (s*):"); println("  shost [name]  Show/set hostname");
    println("  sver          Show system version"); println("");
}

pub fn cls(_: &Cmd) { print("\x1B[2J\x1B[H"); }

pub fn echo(cmd: &Cmd) {
    for i in 1..cmd.n { if i > 1 { print(" "); } print(as_str(cmd.get(i))); }
    println("");
}

pub fn exit(_: &Cmd) {
    println("Goodbye!");
    super::RUNNING.store(false, core::sync::atomic::Ordering::Relaxed);
}
