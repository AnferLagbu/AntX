use core::ffi::c_char;

use super::ramfs::RAMFS_DATA;
use crate::kernel::fs::vfs::types::*;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn ramfs_init() {
    super::ramfs::init();
}

#[no_mangle]
pub extern "C" fn ramfs_mount(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mount(path)
}

#[no_mangle]
pub extern "C" fn ramfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let mut ramfs = RAMFS_DATA.lock();
    match ramfs.open(path, flags, pwid) {
        Some((inode_num, offset, _file_type)) => {
            ((inode_num as i32) & 0xFFFF) | ((offset as i32) << 16)
        }
        None => -1
    }
}

#[no_mangle]
pub extern "C" fn ramfs_read(inode_num: u32, offset: *mut u64, buf: *mut u8, count: u32, pwid: u64) -> i32 {
    if buf.is_null() || offset.is_null() {
        return -1;
    }
    
    let mut ramfs = RAMFS_DATA.lock();
    unsafe {
        let buffer = core::slice::from_raw_parts_mut(buf, count as usize);
        let off = &mut *offset;
        ramfs.read(inode_num, off, buffer, pwid)
    }
}

#[no_mangle]
pub extern "C" fn ramfs_write(inode_num: u32, offset: *mut u64, buf: *const u8, count: u32, pwid: u64) -> i32 {
    if buf.is_null() || offset.is_null() {
        return -1;
    }
    
    let mut ramfs = RAMFS_DATA.lock();
    unsafe {
        let buffer = core::slice::from_raw_parts(buf, count as usize);
        let off = &mut *offset;
        ramfs.write(inode_num, off, buffer, pwid)
    }
}

#[no_mangle]
pub extern "C" fn ramfs_mkdir(parent_path: *const c_char, name: *const c_char, pwid: u64) -> i32 {
    let parent_path = ptr_to_str(parent_path);
    let name = ptr_to_str(name);
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mkdir(parent_path, name, pwid)
}

#[no_mangle]
pub extern "C" fn ramfs_stat(inode_num: u32, st: *mut VfsStat) -> i32 {
    if st.is_null() {
        return -1;
    }
    
    let ramfs = RAMFS_DATA.lock();
    match ramfs.stat(inode_num) {
        Some(stat) => {
            unsafe { *st = stat; }
            0
        }
        None => -1
    }
}

#[no_mangle]
pub extern "C" fn ramfs_resolve_path(path: *const c_char) -> u32 {
    let path = ptr_to_str(path);
    let ramfs = RAMFS_DATA.lock();
    match ramfs.resolve_path(path) {
        Some(num) => num,
        None => 0
    }
}
