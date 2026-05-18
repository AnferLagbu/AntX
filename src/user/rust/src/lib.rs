#![no_std]

pub mod syscall;
pub mod install_wizard;

pub use syscall::*;

pub fn print(s: &str) {
    syscall::fs_write(1, s.as_bytes());
}

pub fn println(s: &str) {
    print(s);
    print("\n");
}

pub fn print_char(c: u8) {
    syscall::fs_write(1, &[c]);
}

pub fn print_hex(mut val: u64) {
    let mut buf = [b'0'; 16];
    for i in (0..16).rev() {
        let d = (val & 0xF) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'A' + d - 10 };
        val >>= 4;
    }
    print("0x");
    syscall::fs_write(1, &buf);
}

pub fn print_dec(mut val: i64) {
    let mut buf = [0u8; 21];
    let mut i = 20;
    let neg = val < 0;
    if neg { val = -val; }
    if val == 0 {
        i -= 1; buf[i] = b'0';
    } else {
        while val > 0 {
            i -= 1; buf[i] = b'0' + (val % 10) as u8;
            val /= 10;
        }
    }
    if neg { i -= 1; buf[i] = b'-'; }
    syscall::fs_write(1, &buf[i..]);
}

pub fn read_line(buf: &mut [u8]) -> usize {
    let max = buf.len();
    let mut i = 0;
    while i < max.saturating_sub(1) {
        let mut c = 0u8;
        let n = syscall::fs_read(0, core::slice::from_mut(&mut c));
        if n <= 0 { continue; }
        if c == b'\n' { print("\n"); break; }
        else if c == 0x7F || c == 0x08 {
            if i > 0 { i -= 1; print("\x08 \x08"); }
        } else if c >= b' ' && c <= b'~' {
            buf[i] = c; i += 1; print_char(c);
        }
    }
    if i < max { buf[i] = 0; }
    i
}

pub fn cmp(a: &[u8], b: &[u8]) -> i32 {
    let end = if a.len() < b.len() { a.len() } else { b.len() };
    for i in 0..end {
        let d = a[i] as i32 - b[i] as i32;
        if d != 0 { return d; }
    }
    a.len() as i32 - b.len() as i32
}

struct ParseBuf { ptrs: [*const u8; 32], buf: [u8; 256] }
static mut PARSE: ParseBuf = ParseBuf { ptrs: [core::ptr::null(); 32], buf: [0u8; 256] };

pub fn parse_args(line: &[u8]) -> (&'static [*const u8], usize) {
    let parse = unsafe { &mut *core::ptr::addr_of_mut!(PARSE) };
    let mut argc: usize = 0;
    let mut in_arg = false;
    let mut in_quote = false;
    let mut out = 0usize;

    let mut iter = line.iter().peekable();
    loop {
        let byte = match iter.next() { Some(&b) => b, None => 0u8 };
        if byte == 0 { break; }
        if byte == b'"' { in_quote = !in_quote; }
        else if byte == b' ' && !in_quote {
            if in_arg { parse.buf[out] = 0; out += 1; in_arg = false; }
        } else {
            if !in_arg && argc < 31 {
                parse.ptrs[argc] = parse.buf[out..].as_ptr();
                argc += 1; in_arg = true;
            }
            if out < parse.buf.len() { parse.buf[out] = byte; out += 1; }
        }
    }
    if in_arg && out < parse.buf.len() { parse.buf[out] = 0; }
    parse.ptrs[argc] = core::ptr::null();
    (&parse.ptrs[..argc + 1], argc)
}

pub fn delay_loop(count: u64) {
    for _ in 0..count { core::hint::spin_loop(); }
}

pub fn file_open(path: &[u8], flags: i32) -> i32 {
    let mut p = [0u8; 256];
    let len = core::cmp::min(path.len(), 255);
    p[..len].copy_from_slice(&path[..len]);
    p[len] = 0;
    syscall::fs_open(&p[..len + 1], flags, 0o644)
}

pub fn file_copy(src_path: &[u8], dst_path: &[u8]) -> bool {
    let rfd = file_open(src_path, O_RDONLY);
    if rfd < 0 { return false; }
    let wfd = file_open(dst_path, O_CREAT | O_WRONLY);
    if wfd < 0 { syscall::fs_close(rfd); return false; }
    let mut buf = [0u8; 4096];
    loop {
        let n = syscall::fs_read(rfd, &mut buf);
        if n <= 0 { break; }
        syscall::fs_write(wfd, &buf[..n as usize]);
    }
    syscall::fs_close(rfd);
    syscall::fs_close(wfd);
    true
}
