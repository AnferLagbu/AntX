use core::ffi::c_char;

use super::devfs::DEVFS_DATA;

const MAX_PATH_LEN: usize = 1024;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..MAX_PATH_LEN).find(|&i| *ptr.add(i) == 0).unwrap_or(MAX_PATH_LEN);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn devfs_init() {
    super::devfs::init();
}

#[no_mangle]
pub extern "C" fn devfs_mount(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    DEVFS_DATA.mount(path)
}

#[no_mangle]
pub extern "C" fn devfs_open(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    match DEVFS_DATA.open(path) {
        Some((dev_type, _)) => dev_type as i32,
        None => -1
    }
}

#[no_mangle]
pub extern "C" fn devfs_read(dev_type: u8, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    
    unsafe {
        let buffer = core::slice::from_raw_parts_mut(buf, count as usize);
        DEVFS_DATA.read(dev_type, buffer)
    }
}

#[no_mangle]
pub extern "C" fn devfs_write(dev_type: u8, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    
    unsafe {
        let buffer = core::slice::from_raw_parts(buf, count as usize);
        DEVFS_DATA.write(dev_type, buffer)
    }
}

#[no_mangle]
pub extern "C" fn devfs_device_count() -> u32 {
    DEVFS_DATA.device_count()
}
