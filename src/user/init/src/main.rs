//! AntX Init — 首个用户态进程
//!
//! 检测首次启动 → 运行安装向导 → 挂载文件系统 → 启动 axsh Shell

#![no_std]
#![no_main]

use userlib::*;

const FSTAB_PATH: &[u8] = b"/cfg/system/fstab\0";
const SHELL_PATH: &[u8] = b"/app/sys/axsh\0";

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    userlib::print("[init] PANIC: ");
    if let Some(loc) = info.location() {
        userlib::print("at "); userlib::print(loc.file());
        userlib::print(":"); print_dec(loc.line() as i64);
    }
    userlib::print("\n");
    proc_exit(1);
}

fn mount_fstab() {
    let fd = file_open(FSTAB_PATH, O_RDONLY);
    if fd < 0 {
        println("[init] No fstab found, using defaults");
        let mounts: [(&[u8], &[u8]); 3] = [
            (b"/dev\0", b"devfs\0"),
            (b"/proc\0", b"procfs\0"),
            (b"/temp\0", b"ramfs\0"),
        ];
        for (path, fstype) in &mounts {
            print("[init] Mounting "); print(core::str::from_utf8(path).unwrap_or("?").trim_end_matches('\0'));
            print(" ("); print(core::str::from_utf8(fstype).unwrap_or("?").trim_end_matches('\0')); println(")...");
            if fs_mount(b"none\0", path, fstype, b"defaults\0") != 0 {
                println("  [WARN] Failed to mount");
            }
        }
        return;
    }

    println("[init] Reading fstab...");
    let mut mounts_done: u32 = 0;
    let mut line = [0u8; 256];
    let mut pos: usize = 0;

    loop {
        let mut c = 0u8;
        let n = fs_read(fd, core::slice::from_mut(&mut c));
        if n <= 0 { break; }
        if c == b'\n' {
            if pos > 0 && line[0] != b'#' && line[0] != b' ' {
                struct FstabEntry { src: [u8; 64], tgt: [u8; 64], typ: [u8; 32], opt: [u8; 64] }
                let mut e = FstabEntry { src: [0; 64], tgt: [0; 64], typ: [0; 32], opt: [0; 64] };
                let (mut si, mut ti, mut fi, mut oi) = (0usize, 0usize, 0usize, 0usize);
                let mut field: u8 = 0;
                let mut skip = false;
                for i in 0..pos {
                    if line[i] == b' ' || line[i] == b'\t' {
                        if field < 3 && !skip { field += 1; }
                        skip = line[i] == b' ' || line[i] == b'\t';
                    } else {
                        skip = false;
                        match field {
                            0 => { if si < 63 { e.src[si] = line[i]; si += 1; } }
                            1 => { if ti < 63 { e.tgt[ti] = line[i]; ti += 1; } }
                            2 => { if fi < 31 { e.typ[fi] = line[i]; fi += 1; } }
                            _ => { if oi < 63 { e.opt[oi] = line[i]; oi += 1; } }
                        }
                    }
                }
                if ti > 0 && fi > 0 {
                    let tgt_str = core::str::from_utf8(&e.tgt[..ti]).unwrap_or("?");
                    let typ_str = core::str::from_utf8(&e.typ[..fi]).unwrap_or("?");
                    print("[init] Mounting "); print(tgt_str); print(" ("); print(typ_str); println(")...");
                    if fs_mount(&e.src, &e.tgt, &e.typ, &e.opt) == 0 {
                        println("  [OK] Mounted"); mounts_done += 1;
                    } else { println("  [WARN] Failed to mount"); }
                }
            }
            pos = 0;
        } else if pos < 255 { line[pos] = c; pos += 1; }
    }
    fs_close(fd);
    print("[init] Mounted "); print_dec(mounts_done as i64); println(" filesystems from fstab");
}

fn start_shell() {
    println("[init] Starting axsh...");
    let argv: [*const u8; 2] = [b"axsh\0".as_ptr(), core::ptr::null()];
    let r = proc_exec(SHELL_PATH, &argv);
    if r < 0 {
        print("[init] ERROR: Failed to start shell (error: ");
        print_dec(r); println(")");
        println("[init] System halted.");
        loop { proc_yield(); }
    }
}

fn init_main() {
    println("");
    println("[init] AntX init process started");
    println("");

    if install_wizard::check_needed() {
        println("[init] First boot detected, launching installation wizard...");
        println("");
        install_wizard::run();
        println("");
        println("[init] Installation complete, continuing boot...");
        println("");
    }

    mount_fstab();
    println("");
    start_shell();
    loop { proc_yield(); }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    init_main();
    proc_exit(0);
}
