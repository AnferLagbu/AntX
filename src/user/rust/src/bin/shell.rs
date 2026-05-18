//! axsh — AntX Rust Shell
//!
//! 19 条内建命令的交互式 Shell。所有命令通过 int 0x80 syscall 与内核交互。

#![no_std]
#![no_main]

use userlib::*;

static RUNNING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    print("[axsh] PANIC: ");
    if let Some(loc) = info.location() {
        print("at "); print(loc.file());
        print(":"); print_dec(loc.line() as i64);
    }
    print("\n");
    proc_exit(1);
}

fn as_str(ptr: *const u8) -> &'static str {
    if ptr.is_null() { return ""; }
    unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        let slice = core::slice::from_raw_parts(ptr, len);
        core::str::from_utf8_unchecked(slice)
    }
}

fn print_banner() {
    println("");
    println("  ___  _  _ ___");
    println(" / _ \\| || | __|");
    println("| (_) | || |__ \\");
    println(" \\___/|_||_|___/");
    println("");
    println("axsh - AntX Shell");
    println("Type 'help' for commands");
    println("");
}

fn print_prompt() {
    let mut cwd = [0u8; 64];
    let _ = env_getcwd(&mut cwd);
    let cwd_str = core::str::from_utf8(&cwd).unwrap_or("/").trim_end_matches('\0');
    let pwid = proc_get_pwid();
    if pwid != 0 {
        print("["); print_hex(pwid); print("]");
    }
    print(if cwd_str.is_empty() { "/" } else { cwd_str });
    print("> ");
}

// ──── 命令实现 ────────────────────────────────────────────────────────────

fn cmd_help(_args: &[*const u8], _n: usize) {
    println("");
    println("axsh - AntX Shell Commands");
    println("==========================");
    println("");
    println("General:");
    println("  help          Show this help");
    println("  cls           Clear screen");
    println("  echo [text]   Print text");
    println("  exit          Exit shell");
    println("");
    println("File (f*):");
    println("  fls [path]    List directory");
    println("  fcd <dir>     Change directory");
    println("  fpwd          Print working directory");
    println("  fcat <file>   Display file");
    println("  fmk <file>    Create file");
    println("  fmd <dir>     Create directory");
    println("  frm <path>    Remove file/dir");
    println("  fput <f> <t>  Write text to file");
    println("  fsync         Sync to disk");
    println("");
    println("Identity (i*):");
    println("  ilogin <n> <pw>  Login with note and password");
    println("  ilogout         Logout");
    println("  iwho            Show current PWID");
    println("  ipasswd         Change password");
    println("");
    println("System (s*):");
    println("  shost [name]  Show/set hostname");
    println("  sver          Show system version");
    println("");
}

fn cmd_cls(_args: &[*const u8], _n: usize) {
    print("\x1B[2J\x1B[H");
}

fn cmd_echo(args: &[*const u8], n: usize) {
    for i in 1..n {
        if i > 1 { print(" "); }
        print(as_str(args[i]));
    }
    println("");
}

fn cmd_exit(_args: &[*const u8], _n: usize) {
    println("Goodbye!");
    RUNNING.store(false, core::sync::atomic::Ordering::Relaxed);
}

fn cmd_fls(args: &[*const u8], n: usize) {
    let path = if n > 1 { as_str(args[1]) } else { "/" };
    let mut p = [0u8; 256];
    let path_bytes = path.as_bytes();
    let len = path_bytes.len().min(255);
    p[..len].copy_from_slice(&path_bytes[..len]);
    p[len] = 0;

    let fd = file_open(&p[..len + 1], O_RDONLY);
    if fd < 0 {
        print("fls: '"); print(path); println("' not found");
        return;
    }
    let mut count = 0;
    loop {
        let mut entry = UserDirent { inode: 0, file_type: 0, name: [0; 256] };
        if fs_readdir(fd, &mut entry) <= 0 { break; }
        if entry.inode != 0 {
            if entry.file_type == FT_DIR { print("  [D] "); }
            else { print("  [F] "); }
            let name = core::str::from_utf8(&entry.name).unwrap_or("?");
            println(name.trim_end_matches('\0'));
            count += 1;
        }
    }
    fs_close(fd);
    if count == 0 { println("  (empty)"); }
}

fn cmd_fcd(args: &[*const u8], n: usize) {
    if n < 2 { println("fcd: missing path"); return; }
    let path = as_str(args[1]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;
    if env_chdir(&p[..len + 1]) < 0 {
        print("fcd: '"); print(path); println("' not found");
    }
}

fn cmd_fpwd(_args: &[*const u8], _n: usize) {
    let mut cwd = [0u8; 128];
    if env_getcwd(&mut cwd) >= 0 {
        let s = core::str::from_utf8(&cwd).unwrap_or("/").trim_end_matches('\0');
        println(if s.is_empty() { "/" } else { s });
    } else { println("/"); }
}

fn cmd_fcat(args: &[*const u8], n: usize) {
    if n < 2 { println("fcat: missing file"); return; }
    let path = as_str(args[1]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;

    let fd = file_open(&p[..len + 1], O_RDONLY);
    if fd < 0 { print("fcat: '"); print(path); println("' not found"); return; }
    let mut buf = [0u8; 512];
    loop {
        let n = fs_read(fd, &mut buf[..511]);
        if n <= 0 { break; }
        buf[n as usize] = 0;
        let s = core::str::from_utf8(&buf[..n as usize]).unwrap_or("<binary>");
        print(s);
    }
    fs_close(fd);
}

fn cmd_fmk(args: &[*const u8], n: usize) {
    if n < 2 { println("fmk: missing file name"); return; }
    let path = as_str(args[1]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;

    let fd = file_open(&p[..len + 1], O_CREAT | O_WRONLY);
    if fd < 0 { print("fmk: cannot create '"); print(path); println("'"); return; }
    fs_close(fd);
    print("Created: "); println(path);
}

fn cmd_fmd(args: &[*const u8], n: usize) {
    if n < 2 { println("fmd: missing directory name"); return; }
    let path = as_str(args[1]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;

    if fs_mkdir(&p[..len + 1]) < 0 {
        print("fmd: cannot create '"); print(path); println("'"); return;
    }
    print("Created: "); println(path);
}

fn cmd_frm(args: &[*const u8], n: usize) {
    if n < 2 { println("frm: missing path"); return; }
    let path = as_str(args[1]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;

    if fs_unlink(&p[..len + 1]) < 0 {
        print("frm: cannot remove '"); print(path); println("'"); return;
    }
    print("Removed: "); println(path);
}

fn cmd_fput(args: &[*const u8], n: usize) {
    if n < 3 { println("fput: usage: fput <file> <text>"); return; }
    let path = as_str(args[1]);
    let text = as_str(args[2]);
    let mut p = [0u8; 256];
    let pb = path.as_bytes();
    let len = pb.len().min(255);
    p[..len].copy_from_slice(&pb[..len]);
    p[len] = 0;

    let fd = file_open(&p[..len + 1], O_CREAT | O_WRONLY | O_TRUNC);
    if fd < 0 { print("fput: cannot open '"); print(path); println("'"); return; }
    let n = fs_write(fd, text.as_bytes());
    fs_close(fd);
    print("Wrote "); print_dec(n as i64); println(" bytes");
}

fn cmd_fsync(_args: &[*const u8], _n: usize) {
    fs_sync();
    println("Synced");
}

fn cmd_ilogin(args: &[*const u8], n: usize) {
    if n < 3 { println("ilogin: usage: ilogin <note> <password>"); return; }
    let note = as_str(args[1]);
    let pw = as_str(args[2]);
    let mut nb = [0u8; 128]; let nbl = note.as_bytes().len().min(127);
    nb[..nbl].copy_from_slice(&note.as_bytes()[..nbl]); nb[nbl] = 0;
    let mut pb = [0u8; 128]; let pbl = pw.as_bytes().len().min(127);
    pb[..pbl].copy_from_slice(&pw.as_bytes()[..pbl]); pb[pbl] = 0;

    let result = auth_login(&nb[..nbl + 1], &pb[..pbl + 1]);
    if result > 0 {
        print("Logged in: "); print_hex(proc_get_pwid()); print("\n");
    } else if result == -104 {
        println("ilogin: wrong password");
    } else if result == -101 {
        println("ilogin: not found");
    } else {
        println("ilogin: failed");
    }
}

fn cmd_ilogout(_args: &[*const u8], _n: usize) {
    auth_logout();
    println("Logged out");
}

fn cmd_iwho(_args: &[*const u8], _n: usize) {
    let pwid = proc_get_pwid();
    if pwid == 0 { println("Not logged in"); }
    else { print("PWID: "); print_hex(pwid); print("\n"); }
}

fn cmd_ipasswd(_args: &[*const u8], _n: usize) {
    if proc_get_pwid() == 0 { println("Not logged in"); return; }

    print("Current password: ");
    let mut old = [0u8; 64]; read_line(&mut old);
    print("New password: ");
    let mut new = [0u8; 64]; read_line(&mut new);
    print("Confirm: ");
    let mut confirm = [0u8; 64]; let cl = read_line(&mut confirm);

    if cmp(&new[..cl], &confirm[..cl]) != 0 { println("Mismatch"); return; }
    let r = auth_change_password(&old, &new);
    if r == 0 { println("Password changed"); }
    else if r == -104 { println("Wrong current password"); }
    else { println("Failed"); }
}

fn cmd_shost(args: &[*const u8], n: usize) {
    if n == 1 {
        let mut buf = [0u8; 64];
        if gethostname(&mut buf) == 0 {
            let s = core::str::from_utf8(&buf).unwrap_or("?").trim_end_matches('\0');
            println(s);
        } else { println("Error"); }
    } else {
        let name = as_str(args[1]);
        if sethostname(name.as_bytes()) == 0 {
            print("Host: "); println(name);
        } else { println("Error"); }
    }
}

fn cmd_sver(_args: &[*const u8], _n: usize) {
    println("AntX Operating System");
    println("Kernel: QueenX (QX)");
    println("Userland: Rust");
}

// ──── 命令表与分发 ────────────────────────────────────────────────────────

type CmdFn = fn(&[*const u8], usize);

struct Builtin { name: &'static str, func: CmdFn }

static BUILTINS: &[Builtin] = &[
    Builtin { name: "help",    func: cmd_help },
    Builtin { name: "cls",     func: cmd_cls },
    Builtin { name: "echo",    func: cmd_echo },
    Builtin { name: "exit",    func: cmd_exit },
    Builtin { name: "fls",     func: cmd_fls },
    Builtin { name: "fcd",     func: cmd_fcd },
    Builtin { name: "fpwd",    func: cmd_fpwd },
    Builtin { name: "fcat",    func: cmd_fcat },
    Builtin { name: "fmk",     func: cmd_fmk },
    Builtin { name: "fmd",     func: cmd_fmd },
    Builtin { name: "frm",     func: cmd_frm },
    Builtin { name: "fput",    func: cmd_fput },
    Builtin { name: "fsync",   func: cmd_fsync },
    Builtin { name: "ilogin",  func: cmd_ilogin },
    Builtin { name: "ilogout", func: cmd_ilogout },
    Builtin { name: "iwho",    func: cmd_iwho },
    Builtin { name: "ipasswd", func: cmd_ipasswd },
    Builtin { name: "shost",   func: cmd_shost },
    Builtin { name: "sver",    func: cmd_sver },
];

fn execute(args: &[*const u8], argc: usize) {
    if argc == 0 { return; }
    let cmd = as_str(args[0]);
    for b in BUILTINS {
        if cmd == b.name { (b.func)(args, argc); return; }
    }
    print("Unknown: "); println(cmd);
}

fn shell_main() {
    print_banner();
    let mut line = [0u8; 256];

    while RUNNING.load(core::sync::atomic::Ordering::Relaxed) {
        print_prompt();
        let len = read_line(&mut line);
        if len == 0 { continue; }
        let (args, argc) = parse_args(&line[..len]);
        if argc == 0 { continue; }
        execute(args, argc);
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    shell_main();
    proc_exit(0);
}
