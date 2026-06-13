//! Credo v1 持久化存储
//!
//! 二进制格式 v5: 头部 + 条目, 存储路径 `/pwm.db`.
//! 支持从 v4 格式迁移.

use super::identity;
use super::types::*;
use core::sync::atomic::Ordering;

const DB_PATH: &str = "/pwm.db";
const DB_MAGIC: [u8; 4] = *b"PWID";
const DB_VER_MAJOR: u16 = 5;
const DB_VER_MINOR: u16 = 0;

fn as_cstr(p: &[u8]) -> *const u8 {
    p.as_ptr()
}
const ENTRY_SZ: usize = 8 + 8 + 1 + 2 + 128 + PWM_NOTE_LEN + PWM_HASH_LEN + 8 + 8;
const HDR_SZ: usize = 4 + 2 + 2 + 4;

extern "C" {
    fn vfs_open_internal(path: *const u8, flags: u32, pwm: u64) -> i32;
    fn vfs_close_internal(fd_idx: u32) -> i32;
    fn vfs_write_internal(fd_idx: u32, buf: *const u8, count: u32) -> i32;
    fn vfs_read_internal(fd_idx: u32, buf: *mut u8, count: u32) -> i32;
    fn vfs_unlink_internal(path: *const u8, pwm: u64) -> i32;
}

const O_RDONLY: u32 = 0x0001;
const O_WRONLY: u32 = 0x0002;
const O_CREAT: u32 = 0x0100;
const O_TRUNC: u32 = 0x0200;

fn path_to_bytes(s: &str) -> [u8; 128] {
    let mut buf = [0u8; 128];
    let bytes = s.as_bytes();
    let len = bytes.len().min(127);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    buf
}

fn w32(buf: &mut [u8], p: &mut usize, v: u32) {
    buf[*p] = v as u8;
    buf[*p + 1] = (v >> 8) as u8;
    buf[*p + 2] = (v >> 16) as u8;
    buf[*p + 3] = (v >> 24) as u8;
    *p += 4;
}
fn w64(buf: &mut [u8], p: &mut usize, v: u64) {
    for i in 0..8 {
        buf[*p + i] = (v >> (i * 8)) as u8;
    }
    *p += 8;
}
fn w16(buf: &mut [u8], p: &mut usize, v: u16) {
    buf[*p] = v as u8;
    buf[*p + 1] = (v >> 8) as u8;
    *p += 2;
}
fn w8(buf: &mut [u8], p: &mut usize, v: u8) {
    buf[*p] = v;
    *p += 1;
}

fn r32(buf: &[u8], p: &mut usize) -> u32 {
    let v = buf[*p] as u32
        | (buf[*p + 1] as u32) << 8
        | (buf[*p + 2] as u32) << 16
        | (buf[*p + 3] as u32) << 24;
    *p += 4;
    v
}
fn r64(buf: &[u8], p: &mut usize) -> u64 {
    let mut v = 0u64;
    for i in 0..8 {
        v |= (buf[*p + i] as u64) << (i * 8);
    }
    *p += 8;
    v
}
fn r16(buf: &[u8], p: &mut usize) -> u16 {
    let v = buf[*p] as u16 | (buf[*p + 1] as u16) << 8;
    *p += 2;
    v
}
fn r8(buf: &[u8], p: &mut usize) -> u8 {
    let v = buf[*p];
    *p += 1;
    v
}

fn serialize(entry: &PwmEntry, buf: &mut [u8], p: &mut usize) {
    w64(buf, p, entry.pwm.load(Ordering::Acquire));
    w64(buf, p, entry.creator_pwm.load(Ordering::Acquire));
    w8(buf, p, entry.privilege_level.load(Ordering::Acquire));
    w16(buf, p, entry.flags.load(Ordering::Acquire));
    for i in 0..16 {
        w64(buf, p, entry.caps[i].load(Ordering::Acquire));
    }
    buf[*p..*p + PWM_NOTE_LEN].copy_from_slice(&entry.note);
    *p += PWM_NOTE_LEN;
    buf[*p..*p + PWM_HASH_LEN].copy_from_slice(&entry.password_hash);
    *p += PWM_HASH_LEN;
    w64(buf, p, entry.created_time.load(Ordering::Acquire));
    w64(buf, p, entry.expires_at.load(Ordering::Acquire));
}

fn deserialize(
    buf: &[u8],
    p: &mut usize,
) -> Option<(
    u64,
    u64,
    u8,
    u16,
    [u64; 16],
    [u8; PWM_NOTE_LEN],
    [u8; PWM_HASH_LEN],
    u64,
    u64,
)> {
    if *p + ENTRY_SZ > buf.len() {
        return None;
    }
    let pwm = r64(buf, p);
    let creator_pwm = r64(buf, p);
    let privilege_level = r8(buf, p);
    let flags = r16(buf, p);
    let mut caps = [0u64; 16];
    for i in 0..16 {
        caps[i] = r64(buf, p);
    }
    let mut note = [0u8; PWM_NOTE_LEN];
    note.copy_from_slice(&buf[*p..*p + PWM_NOTE_LEN]);
    *p += PWM_NOTE_LEN;
    let mut h = [0u8; PWM_HASH_LEN];
    h.copy_from_slice(&buf[*p..*p + PWM_HASH_LEN]);
    *p += PWM_HASH_LEN;
    let created = r64(buf, p);
    let expires = r64(buf, p);
    Some((
        pwm,
        creator_pwm,
        privilege_level,
        flags,
        caps,
        note,
        h,
        created,
        expires,
    ))
}

pub fn save_database() -> i32 {
    let t = identity::get_table();
    if !t.is_modified() {
        return 0;
    }

    let mut n: usize = 0;
    for i in 0..MAX_PWM_ENTRIES {
        if t.entries[i].is_valid() {
            n += 1;
        }
    }

    let sz = HDR_SZ + n * ENTRY_SZ;
    let mut buf = [0u8; 80000];
    if sz > buf.len() {
        return -1;
    }

    let mut p: usize = 0;
    buf[p] = DB_MAGIC[0];
    buf[p + 1] = DB_MAGIC[1];
    buf[p + 2] = DB_MAGIC[2];
    buf[p + 3] = DB_MAGIC[3];
    p += 4;
    w16(&mut buf, &mut p, DB_VER_MAJOR);
    w16(&mut buf, &mut p, DB_VER_MINOR);
    w32(&mut buf, &mut p, n as u32);

    for i in 0..MAX_PWM_ENTRIES {
        if t.entries[i].is_valid() {
            serialize(&t.entries[i], &mut buf, &mut p);
        }
    }

    let path = path_to_bytes(DB_PATH);
    let flags = O_WRONLY | O_CREAT | O_TRUNC;
    let fd = raw::vfs_open(as_cstr(&path), flags, 0);
    if fd < 0 {
        return -1;
    }

    let written = raw::vfs_write(fd as u32, buf.as_ptr(), sz as u32);
    raw::vfs_close(fd as u32);
    if written as usize != sz {
        return -1;
    }

    t.clear_modified();
    0
}

pub fn load_database() -> i32 {
    let path = path_to_bytes(DB_PATH);
    let fd = raw::vfs_open(as_cstr(&path), O_RDONLY, 0);
    if fd < 0 {
        return 0;
    }

    let mut hdr = [0u8; HDR_SZ];
    let rd = raw::vfs_read(fd as u32, hdr.as_mut_ptr(), HDR_SZ as u32);
    if rd < HDR_SZ as i32 {
        raw::vfs_close(fd as u32);
        return -1;
    }

    if hdr[0] != DB_MAGIC[0]
        || hdr[1] != DB_MAGIC[1]
        || hdr[2] != DB_MAGIC[2]
        || hdr[3] != DB_MAGIC[3]
    {
        raw::vfs_close(fd as u32);
        return -1;
    }

    let mut hp: usize = 4;
    let vmaj = r16(&hdr, &mut hp);
    let _vmin = r16(&hdr, &mut hp);
    let count = r32(&hdr, &mut hp) as usize;
    if count == 0 || count > MAX_PWM_ENTRIES {
        raw::vfs_close(fd as u32);
        return -1;
    }

    let ds = count
        * if vmaj < 5 {
            8 + 1 + 2 + 128 + 128 + 48 + 8 + 8
        } else {
            ENTRY_SZ
        };
    let mut data = [0u8; 80000];
    let dr = raw::vfs_read(fd as u32, data.as_mut_ptr(), ds as u32);
    raw::vfs_close(fd as u32);
    if dr < ds as i32 {
        return -1;
    }

    let t = raw::table_mut();
    let mut p: usize = 0;

    if vmaj < 5 {
        let v4_entry_sz = 8 + 1 + 2 + 128 + 128 + 48 + 8 + 8;
        for _ in 0..count {
            if p + v4_entry_sz > data.len() {
                break;
            }
            let pwm = r64(&data, &mut p);
            let level = r8(&data, &mut p);
            let flags = r16(&data, &mut p);
            let mut caps = [0u64; 16];
            for i in 0..16 {
                caps[i] = r64(&data, &mut p);
            }
            let mut note = [0u8; 128];
            note.copy_from_slice(&data[p..p + 128]);
            p += 128;
            let mut h = [0u8; PWM_HASH_LEN];
            h.copy_from_slice(&data[p..p + PWM_HASH_LEN]);
            p += PWM_HASH_LEN;
            let created = r64(&data, &mut p);
            let expires = r64(&data, &mut p);

            for i in 0..MAX_PWM_ENTRIES {
                if !t.entries[i].is_valid() {
                    let e = &mut t.entries[i];
                    e.pwm.store(pwm, Ordering::Release);
                    e.creator_pwm.store(0, Ordering::Release);
                    e.privilege_level.store(level, Ordering::Release);
                    e.flags.store(flags, Ordering::Release);
                    for j in 0..16 {
                        e.caps[j].store(caps[j], Ordering::Release);
                    }
                    let note_trunc = note.len().min(PWM_NOTE_LEN);
                    let zero_pos = note_trunc.min(PWM_NOTE_LEN - 1);
                    e.note[..zero_pos].copy_from_slice(&note[..zero_pos]);
                    e.note[zero_pos] = 0;
                    e.password_hash[..PWM_HASH_LEN].copy_from_slice(&h);
                    e.created_time.store(created, Ordering::Release);
                    e.expires_at.store(expires, Ordering::Release);
                    t.count.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
        }
    } else {
        for _ in 0..count {
            if let Some((
                pwm,
                creator_pwm,
                privilege_level,
                flags,
                caps,
                note,
                h,
                created,
                expires,
            )) = deserialize(&data, &mut p)
            {
                for i in 0..MAX_PWM_ENTRIES {
                    if !t.entries[i].is_valid() {
                        let e = &mut t.entries[i];
                        e.pwm.store(pwm, Ordering::Release);
                        e.creator_pwm.store(creator_pwm, Ordering::Release);
                        e.privilege_level.store(privilege_level, Ordering::Release);
                        e.flags.store(flags, Ordering::Release);
                        for j in 0..16 {
                            e.caps[j].store(caps[j], Ordering::Release);
                        }
                        e.note[..PWM_NOTE_LEN].copy_from_slice(&note[..PWM_NOTE_LEN]);
                        e.password_hash[..PWM_HASH_LEN].copy_from_slice(&h);
                        e.created_time.store(created, Ordering::Release);
                        e.expires_at.store(expires, Ordering::Release);
                        t.count.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }
    }

    if t.count.load(Ordering::Acquire) > 0 {
        t.any_identity_exists.store(true, Ordering::Release);
    }
    t.clear_modified();
    0
}

pub fn remove_database() -> i32 {
    let path = path_to_bytes(DB_PATH);
    raw::vfs_unlink(as_cstr(&path), 0)
}

// ============================================================================
// 特权子模块 (Framekernel raw): 集中 VFS FFI 与身份表访问
// ============================================================================

pub(crate) mod raw {
    use super::*;

    /// VFS open 包装 (调用方负责 path 指针有效)
    pub fn vfs_open(path: *const u8, flags: u32, pwm: u64) -> i32 {
        // SAFETY: 调用方契约: path 指向以 NUL 结尾的合法 C 字符串,
        // flags/pwm 为合法标志字。
        unsafe { vfs_open_internal(path, flags, pwm) }
    }

    pub fn vfs_close(fd: u32) -> i32 {
        // SAFETY: fd 由 vfs_open 返回的有效描述符。
        unsafe { vfs_close_internal(fd) }
    }

    pub fn vfs_write(fd: u32, buf: *const u8, count: u32) -> i32 {
        // SAFETY: 调用方契约: buf 在 write 期间有效, count 正确。
        unsafe { vfs_write_internal(fd, buf, count) }
    }

    pub fn vfs_read(fd: u32, buf: *mut u8, count: u32) -> i32 {
        // SAFETY: 调用方契约: buf 在 read 期间有效, count 正确。
        unsafe { vfs_read_internal(fd, buf, count) }
    }

    pub fn vfs_unlink(path: *const u8, pwm: u64) -> i32 {
        // SAFETY: path 指向以 NUL 结尾的合法 C 字符串。
        unsafe { vfs_unlink_internal(path, pwm) }
    }

    /// 安全访问 identity 表 (读视图)
    #[allow(dead_code)]
    pub fn table() -> &'static super::identity::IdentityTable {
        super::identity::raw::get_table()
    }

    /// 安全访问 identity 表 (写视图, 外部互斥)
    pub fn table_mut() -> &'static mut super::identity::IdentityTable {
        super::identity::raw::get_table_mut()
    }
}
