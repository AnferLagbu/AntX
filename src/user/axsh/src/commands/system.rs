/// 系统命令: shost, sver

use userlib::{print, println, gethostname, sethostname};

use super::{Cmd, as_str};

pub fn shost(cmd: &Cmd) {
    if cmd.n == 1 {
        let mut buf = [0u8; 64];
        if gethostname(&mut buf) == 0 {
            println(core::str::from_utf8(&buf).unwrap_or("?").trim_end_matches('\0'));
        } else { println("Error"); }
    } else {
        let name = as_str(cmd.get(1));
        if sethostname(name.as_bytes()) == 0 { print("Host: "); println(name); }
        else { println("Error"); }
    }
}

pub fn sver(_: &Cmd) {
    println("AntX Operating System"); println("Kernel: QueenX (QX)"); println("Userland: Rust");
}
