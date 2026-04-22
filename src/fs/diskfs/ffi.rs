use core::ffi::c_char;

use super::diskfs::{get_diskfs, DISKFS_DATA};
use crate::fs::vfs::types::*;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_init() {
    super::diskfs::init();
}

#[no_mangle]
pub extern "C" fn rust_diskfs_is_mounted() -> i32 {
    let diskfs = get_diskfs().lock();
    if diskfs.is_mounted() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_mount(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    let mut diskfs = get_diskfs().lock();
    diskfs.mount(path)
}

#[no_mangle]
pub extern "C" fn rust_diskfs_unmount() -> i32 {
    let mut diskfs = get_diskfs().lock();
    diskfs.unmount()
}

#[no_mangle]
pub extern "C" fn rust_diskfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let mut diskfs = get_diskfs().lock();
    match diskfs.open(path, flags, pwid) {
        Some((inode_num, offset, file_type)) => {
            ((inode_num as i32) & 0xFFFF) | ((offset as i32) << 16)
        }
        None => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_close(fd: u32) -> i32 {
    let mut diskfs = get_diskfs().lock();
    diskfs.close(fd)
}

#[no_mangle]
pub extern "C" fn rust_diskfs_read(fd: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    
    let mut diskfs = get_diskfs().lock();
    unsafe {
        let buffer = core::slice::from_raw_parts_mut(buf, count as usize);
        diskfs.read(fd, buffer, count)
    }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_write(fd: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    
    let mut diskfs = get_diskfs().lock();
    unsafe {
        let buffer = core::slice::from_raw_parts(buf, count as usize);
        diskfs.write(fd, buffer, count)
    }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_mkdir(parent_path: *const c_char, name: *const c_char, pwid: u64) -> i32 {
    let parent_path = ptr_to_str(parent_path);
    let name = ptr_to_str(name);
    let mut diskfs = get_diskfs().lock();
    diskfs.mkdir(parent_path, name, pwid)
}

#[no_mangle]
pub extern "C" fn rust_diskfs_stat(path: *const c_char, st: *mut VfsStat, pwid: u64) -> i32 {
    if st.is_null() {
        return -1;
    }
    
    let path = ptr_to_str(path);
    let diskfs = get_diskfs().lock();
    match diskfs.stat(path, pwid) {
        Some(stat) => {
            unsafe { *st = stat; }
            0
        }
        None => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_diskfs_sync() -> i32 {
    let diskfs = get_diskfs().lock();
    diskfs.sync()
}
