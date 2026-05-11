//! Persistent Storage Interface
//!
//! Handles PWID database persistence through the VFS layer.
//! Binary format: header + entries array, stored at /pwid.db

use super::types::*;
use super::manager;
use core::sync::atomic::Ordering;

/// Database file path (root-level for simple access during boot)
const DB_PATH: &str = "/pwid.db";

/// Magic bytes for PWID database file
const DB_MAGIC: [u8; 4] = [b'P', b'W', b'I', b'D'];
const DB_VER_MAJOR: u16 = 4;
const DB_VER_MINOR: u16 = 0;

/// Serialized entry size: pwid(8)+level(1)+flags(2)+caps(128)+note(32)+hash(48)+created(8)+expires(8)
const ENTRY_SZ: usize = 8 + 1 + 2 + 128 + PWID_NOTE_LEN + PWID_HASH_LEN + 8 + 8;
const HDR_SZ: usize = 4 + 2 + 2 + 4;  // magic(4) + maj(2) + min(2) + count(4)

extern "C" {
    fn vfs_open_internal(path: *const core::ffi::c_char, flags: u32, pwid: u64) -> i32;
    fn vfs_close_internal(fd_idx: u32) -> i32;
    fn vfs_write_internal(fd_idx: u32, buf: *const u8, count: u32) -> i32;
    fn vfs_read_internal(fd_idx: u32, buf: *mut u8, count: u32) -> i32;
    fn vfs_mkdir_internal(path: *const core::ffi::c_char, pwid: u64) -> i32;
    fn vfs_unlink_internal(path: *const core::ffi::c_char, pwid: u64) -> i32;
}

const O_RDONLY: u32 = 0x0001;
const O_WRONLY: u32 = 0x0002;
const O_CREAT: u32  = 0x0100;
const O_TRUNC: u32  = 0x0200;

fn path_to_bytes(s: &str) -> [u8; 128] {
    let mut buf = [0u8; 128];
    let bytes = s.as_bytes();
    let len = bytes.len().min(127);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf[len] = 0;
    buf
}

fn w32(buf: &mut [u8], p: &mut usize, v: u32) { buf[*p]=v as u8;buf[*p+1]=(v>>8)as u8;buf[*p+2]=(v>>16)as u8;buf[*p+3]=(v>>24)as u8;*p+=4; }
fn w64(buf: &mut [u8], p: &mut usize, v: u64) { for i in 0..8{buf[*p+i]=(v>>(i*8))as u8;} *p+=8; }
fn w16(buf: &mut [u8], p: &mut usize, v: u16) { buf[*p]=v as u8;buf[*p+1]=(v>>8)as u8;*p+=2; }
fn w8(buf: &mut [u8], p: &mut usize, v: u8)  { buf[*p]=v;*p+=1; }

fn r32(buf: &[u8], p: &mut usize) -> u32 { let v=buf[*p]as u32|(buf[*p+1]as u32)<<8|(buf[*p+2]as u32)<<16|(buf[*p+3]as u32)<<24;*p+=4;v }
fn r64(buf: &[u8], p: &mut usize) -> u64 { let mut v=0u64;for i in 0..8{v|=(buf[*p+i]as u64)<<(i*8);}*p+=8;v }
fn r16(buf: &[u8], p: &mut usize) -> u16 { let v=buf[*p]as u16|(buf[*p+1]as u16)<<8;*p+=2;v }
fn r8(buf: &[u8], p: &mut usize) -> u8 { let v=buf[*p];*p+=1;v }

fn serialize(entry: &super::types::PwidEntry, buf: &mut [u8], p: &mut usize) {
    use core::sync::atomic::Ordering;
    w64(buf, p, entry.pwid.load(Ordering::Acquire));
    w8(buf, p, entry.level.load(Ordering::Acquire));
    w16(buf, p, entry.flags.load(Ordering::Acquire));
    for d in entry.capability_mask.iter() { w64(buf, p, *d); }
    buf[*p..*p+PWID_NOTE_LEN].copy_from_slice(&entry.note); *p+=PWID_NOTE_LEN;
    buf[*p..*p+PWID_HASH_LEN].copy_from_slice(&entry.password_hash); *p+=PWID_HASH_LEN;
    w64(buf, p, entry.created_time.load(Ordering::Acquire));
    w64(buf, p, entry.expires_at.load(Ordering::Acquire));
}

fn deserialize(buf: &[u8], p: &mut usize) -> Option<(u64,u8,u16,[u64;16],[u8;PWID_NOTE_LEN],[u8;PWID_HASH_LEN],u64,u64)> {
    if *p + ENTRY_SZ > buf.len() { return None; }
    let pwid = r64(buf, p); let level = r8(buf, p); let flags = r16(buf, p);
    let mut caps = [0u64; 16]; for i in 0..16 { caps[i] = r64(buf, p); }
    let mut note = [0u8; PWID_NOTE_LEN]; note.copy_from_slice(&buf[*p..*p+PWID_NOTE_LEN]); *p+=PWID_NOTE_LEN;
    let mut h = [0u8; PWID_HASH_LEN]; h.copy_from_slice(&buf[*p..*p+PWID_HASH_LEN]); *p+=PWID_HASH_LEN;
    let created = r64(buf, p); let expires = r64(buf, p);
    Some((pwid,level,flags,caps,note,h,created,expires))
}

/// Save PWID database to disk via VFS
pub fn save_database() -> i32 {
    let mgr = manager::get_manager();
    if !mgr.is_modified() { return 0; }

    let mut n: usize = 0;
    for i in 0..MAX_PWID_ENTRIES { if mgr.entries[i].is_valid() { n+=1; } }

    let sz = HDR_SZ + n * ENTRY_SZ;
    let mut buf = [0u8; 60000];
    if sz > buf.len() { return -1; }

    let mut p: usize = 0;
    buf[p]=DB_MAGIC[0];buf[p+1]=DB_MAGIC[1];buf[p+2]=DB_MAGIC[2];buf[p+3]=DB_MAGIC[3]; p+=4;
    w16(&mut buf, &mut p, DB_VER_MAJOR);
    w16(&mut buf, &mut p, DB_VER_MINOR);
    w32(&mut buf, &mut p, n as u32);

    for i in 0..MAX_PWID_ENTRIES {
        if mgr.entries[i].is_valid() { serialize(&mgr.entries[i], &mut buf, &mut p); }
    }

    let path = path_to_bytes(DB_PATH);
    let flags = O_WRONLY | O_CREAT | O_TRUNC;
    let fd = unsafe { vfs_open_internal(path.as_ptr() as *const i8, flags, 0) };
    if fd < 0 { return -1; }

    let written = unsafe { vfs_write_internal(fd as u32, buf.as_ptr(), sz as u32) };
    unsafe { vfs_close_internal(fd as u32); }
    if written as usize != sz { return -1; }

    mgr.clear_modified();
    0
}

/// Load PWID database from disk via VFS
pub fn load_database() -> i32 {
    let path = path_to_bytes(DB_PATH);
    let fd = unsafe { vfs_open_internal(path.as_ptr() as *const i8, O_RDONLY, 0) };
    if fd < 0 { return 0; }  // No DB — first boot

    let mut hdr = [0u8; HDR_SZ];
    let rd = unsafe { vfs_read_internal(fd as u32, hdr.as_mut_ptr(), HDR_SZ as u32) };
    if rd < HDR_SZ as i32 { unsafe{vfs_close_internal(fd as u32);} return -1; }

    if hdr[0]!=DB_MAGIC[0]||hdr[1]!=DB_MAGIC[1]||hdr[2]!=DB_MAGIC[2]||hdr[3]!=DB_MAGIC[3]
        { unsafe{vfs_close_internal(fd as u32);} return -1; }

    let mut hp: usize = 4;
    let _vmaj = r16(&hdr, &mut hp); let _vmin = r16(&hdr, &mut hp);
    let count = r32(&hdr, &mut hp) as usize;
    if count == 0 || count > MAX_PWID_ENTRIES { unsafe{vfs_close_internal(fd as u32);} return -1; }

    let ds = count * ENTRY_SZ;
    let mut data = [0u8; 60000];
    let dr = unsafe { vfs_read_internal(fd as u32, data.as_mut_ptr(), ds as u32) };
    unsafe { vfs_close_internal(fd as u32); }
    if dr < ds as i32 { return -1; }

    let mgr = unsafe { manager::get_manager_mut() };
    let mut p: usize = 0;
    for _ in 0..count {
        if let Some((pwid,level,flags,caps,note,hash,created,expires)) = deserialize(&data, &mut p) {
            if let Some(slot) = mgr.find_free_slot_lockless() {
                unsafe {
                    let ep = mgr.entries.as_ptr() as *mut super::types::PwidEntry;
                    let e = &mut *ep.add(slot);
                    e.pwid.store(pwid, Ordering::Release);
                    e.level.store(level, Ordering::Release);
                    e.flags.store(flags, Ordering::Release);
                    e.capability_mask.copy_from_slice(&caps);
                    e.note.copy_from_slice(&note);
                    e.password_hash.copy_from_slice(&hash);
                    e.created_time.store(created, Ordering::Release);
                    e.expires_at.store(expires, Ordering::Release);
                }
                mgr.count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    if mgr.count.load(Ordering::Acquire) > 0 {
        mgr.any_identity_exists.store(true, Ordering::Release);
    }
    mgr.clear_modified();
    0
}

pub fn remove_database() -> i32 {
    let path = path_to_bytes(DB_PATH);
    unsafe { vfs_unlink_internal(path.as_ptr() as *const i8, 0) }
}
