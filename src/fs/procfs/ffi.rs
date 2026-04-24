use core::ffi::c_char;

use super::procfs::PROCFS_DATA;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn procfs_init() {
    super::procfs::init();
}

#[no_mangle]
pub extern "C" fn procfs_mount(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    PROCFS_DATA.mount(path)
}

#[no_mangle]
pub extern "C" fn procfs_add_process(pid: u32, name: *const c_char) -> i32 {
    let name = ptr_to_str(name);
    PROCFS_DATA.add_process(pid, name)
}

#[no_mangle]
pub extern "C" fn procfs_remove_process(pid: u32) -> i32 {
    PROCFS_DATA.remove_process(pid)
}

#[no_mangle]
pub extern "C" fn procfs_read(name: *const c_char, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    
    let name = ptr_to_str(name);
    unsafe {
        let buffer = core::slice::from_raw_parts_mut(buf, count as usize);
        PROCFS_DATA.read(name, buffer)
    }
}

#[no_mangle]
pub extern "C" fn procfs_entry_count() -> u32 {
    PROCFS_DATA.entry_count()
}
