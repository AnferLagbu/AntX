/// 命令表、分发调度与共享工具函数
///
/// 子模块:
///   general  — help, cls, echo, exit
///   fileops  — fls, fcd, fpwd, fcat, fmk, fmd, frm, fput, fsync
///   identity — ilogin, ilogout, iwho, ipasswd
///   system   — shost, sver

mod general;
mod fileops;
mod identity;
mod system;

use userlib::{print, println};

use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) static RUNNING: AtomicBool = AtomicBool::new(true);

pub fn is_running() -> bool { RUNNING.load(Ordering::Relaxed) }

pub struct Cmd { argv: *const *const u8, n: usize }

impl Cmd {
    pub fn get(&self, i: usize) -> *const u8 {
        if i >= self.n { return core::ptr::null(); }
        unsafe { *self.argv.add(i) }
    }
}

pub fn as_str(ptr: *const u8) -> &'static str {
    if ptr.is_null() { return ""; }
    unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len))
    }
}

pub fn path_arg(args: &Cmd) -> Option<[u8; 256]> {
    if args.n < 2 { return None; }
    let s = as_str(args.get(1)).as_bytes();
    let len = core::cmp::min(s.len(), 255);
    let mut p = [0u8; 256];
    p[..len].copy_from_slice(&s[..len]); p[len] = 0;
    Some(p)
}

type CmdFn = fn(&Cmd);

struct Builtin { name: &'static str, func: CmdFn }

static BUILTINS: &[Builtin] = &[
    Builtin { name: "help",    func: general::help },
    Builtin { name: "cls",     func: general::cls },
    Builtin { name: "echo",    func: general::echo },
    Builtin { name: "exit",    func: general::exit },
    Builtin { name: "fls",     func: fileops::fls },
    Builtin { name: "fcd",     func: fileops::fcd },
    Builtin { name: "fpwd",    func: fileops::fpwd },
    Builtin { name: "fcat",    func: fileops::fcat },
    Builtin { name: "fmk",     func: fileops::fmk },
    Builtin { name: "fmd",     func: fileops::fmd },
    Builtin { name: "frm",     func: fileops::frm },
    Builtin { name: "fput",    func: fileops::fput },
    Builtin { name: "fsync",   func: fileops::fsync },
    Builtin { name: "ilogin",  func: identity::ilogin },
    Builtin { name: "ilogout", func: identity::ilogout },
    Builtin { name: "iwho",    func: identity::iwho },
    Builtin { name: "ipasswd", func: identity::ipasswd },
    Builtin { name: "shost",   func: system::shost },
    Builtin { name: "sver",    func: system::sver },
];

pub fn dispatch(args: &[*const u8], argc: usize) {
    if argc == 0 { return; }
    let cmd_name = as_str(args[0]);
    let cmd = Cmd { argv: args.as_ptr(), n: argc };
    for b in BUILTINS { if cmd_name == b.name { (b.func)(&cmd); return; } }
    print("Unknown: "); println(cmd_name);
}
