/// 文件操作命令: fls, fcd, fpwd, fcat, fmk, fmd, frm, fput, fsync

use userlib::*;

use super::{Cmd, as_str, path_arg};

fn print_nb(buf: &[u8]) { print(core::str::from_utf8(buf).unwrap_or("?").trim_end_matches('\0')); }

pub fn fls(cmd: &Cmd) {
    let p = path_arg(cmd).unwrap_or_else(|| { let mut d = [0u8; 256]; d[0]=b'/'; d[1]=0; d });
    let fd = file_open(&p, O_RDONLY);
    if fd < 0 { print("fls: '"); print_nb(&p); println("' not found"); return; }
    let mut count = 0;
    loop {
        let mut entry = UserDirent { inode: 0, file_type: 0, name: [0; 256] };
        if fs_readdir(fd, &mut entry) <= 0 { break; }
        if entry.inode != 0 {
            if entry.file_type == FT_DIR { print("  [D] "); } else { print("  [F] "); }
            println(core::str::from_utf8(&entry.name).unwrap_or("?").trim_end_matches('\0'));
            count += 1;
        }
    }
    fs_close(fd);
    if count == 0 { println("  (empty)"); }
}

pub fn fcd(cmd: &Cmd) {
    if let Some(p) = path_arg(cmd) {
        if env_chdir(&p) < 0 { print("fcd: '"); print_nb(&p); println("' not found"); }
    } else { println("fcd: missing path"); }
}

pub fn fpwd(_: &Cmd) {
    let mut cwd = [0u8; 128];
    if env_getcwd(&mut cwd) >= 0 {
        let s = core::str::from_utf8(&cwd).unwrap_or("/").trim_end_matches('\0');
        println(if s.is_empty() { "/" } else { s });
    } else { println("/"); }
}

pub fn fcat(cmd: &Cmd) {
    if let Some(p) = path_arg(cmd) {
        let fd = file_open(&p, O_RDONLY);
        if fd < 0 { print("fcat: '"); print_nb(&p); println("' not found"); return; }
        let mut buf = [0u8; 512];
        loop { let n = fs_read(fd, &mut buf[..511]); if n <= 0 { break; } buf[n as usize]=0;
            print(core::str::from_utf8(&buf[..n as usize]).unwrap_or("<binary>")); }
        fs_close(fd);
    } else { println("fcat: missing file"); }
}

pub fn fmk(cmd: &Cmd) {
    if let Some(p) = path_arg(cmd) {
        let fd = file_open(&p, O_CREAT | O_WRONLY);
        if fd < 0 { print("fmk: cannot create '"); print_nb(&p); println("'"); return; }
        fs_close(fd); print("Created: "); print_nb(&p); println("");
    } else { println("fmk: missing file name"); }
}

pub fn fmd(cmd: &Cmd) {
    if let Some(p) = path_arg(cmd) {
        if fs_mkdir(&p) < 0 { print("fmd: cannot create '"); print_nb(&p); println("'"); return; }
        print("Created: "); print_nb(&p); println("");
    } else { println("fmd: missing directory name"); }
}

pub fn frm(cmd: &Cmd) {
    if let Some(p) = path_arg(cmd) {
        if fs_unlink(&p) < 0 { print("frm: cannot remove '"); print_nb(&p); println("'"); return; }
        print("Removed: "); print_nb(&p); println("");
    } else { println("frm: missing path"); }
}

pub fn fput(cmd: &Cmd) {
    if cmd.n < 3 { println("fput: usage: fput <file> <text>"); return; }
    let path = as_str(cmd.get(1)); let text_str = as_str(cmd.get(2));
    let mut p = [0u8; 256]; let pb = path.as_bytes(); let len = core::cmp::min(pb.len(), 255);
    p[..len].copy_from_slice(&pb[..len]); p[len] = 0;
    let fd = file_open(&p, O_CREAT | O_WRONLY | O_TRUNC);
    if fd < 0 { print("fput: cannot open '"); print(path); println("'"); return; }
    let n = fs_write(fd, text_str.as_bytes()); fs_close(fd);
    print("Wrote "); print_dec(n as i64); println(" bytes");
}

pub fn fsync(_: &Cmd) { userlib::fs_sync(); println("Synced"); }
