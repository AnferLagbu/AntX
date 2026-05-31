use core::ffi::c_char;

use super::ramfs::RAMFS_DATA;
use crate::kernel::fs::vfs::types::*;

const MAX_PATH_LEN: usize = 1024;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let len = (0..MAX_PATH_LEN)
            .find(|&i| *ptr.add(i) == 0)
            .unwrap_or(MAX_PATH_LEN);
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
pub extern "C" fn ramfs_open(path: *const c_char, flags: u32, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let mut ramfs = RAMFS_DATA.lock();
    match ramfs.open(path, flags, pwm) {
        Some((node_id, offset, _file_type)) => {
            ((node_id as i32) & 0xFFFF) | ((offset as i32) << 16)
        }
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn ramfs_read(
    node_id: u32,
    offset: *mut u64,
    buf: *mut u8,
    count: u32,
    pwm: u64,
) -> i32 {
    if buf.is_null() || offset.is_null() {
        return -1;
    }

    let mut ramfs = RAMFS_DATA.lock();
    unsafe {
        let buffer = core::slice::from_raw_parts_mut(buf, count as usize);
        let off = &mut *offset;
        ramfs.read(node_id, off, buffer, pwm)
    }
}

#[no_mangle]
pub extern "C" fn ramfs_write(
    node_id: u32,
    offset: *mut u64,
    buf: *const u8,
    count: u32,
    pwm: u64,
) -> i32 {
    if buf.is_null() || offset.is_null() {
        return -1;
    }

    let mut ramfs = RAMFS_DATA.lock();
    unsafe {
        let buffer = core::slice::from_raw_parts(buf, count as usize);
        let off = &mut *offset;
        ramfs.write(node_id, off, buffer, pwm)
    }
}

#[no_mangle]
pub extern "C" fn ramfs_mkdir(parent_path: *const c_char, name: *const c_char, pwm: u64) -> i32 {
    let parent_path = ptr_to_str(parent_path);
    let name = ptr_to_str(name);
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.mkdir(parent_path, name, pwm)
}

#[no_mangle]
pub extern "C" fn ramfs_stat(node_id: u32, st: *mut VfsStat) -> i32 {
    if st.is_null() {
        return -1;
    }

    let ramfs = RAMFS_DATA.lock();
    match ramfs.stat(node_id) {
        Some(stat) => {
            unsafe {
                *st = stat;
            }
            0
        }
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn ramfs_resolve_path(path: *const c_char) -> u32 {
    let path = ptr_to_str(path);
    let ramfs = RAMFS_DATA.lock();
    match ramfs.resolve_path(path) {
        Some(num) => num,
        None => 0,
    }
}
