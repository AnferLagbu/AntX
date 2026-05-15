use core::ffi::c_char;

#[no_mangle]
pub extern "C" fn zvfs_init() -> i32 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().init();
    0
}

#[no_mangle]
pub extern "C" fn zvfs_mount(_path: *const c_char) -> i32 {
    let zvfs = crate::kernel::fs::zvfs::zvfs::get_zvfs();
    if zvfs.is_initialized() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn zvfs_unmount() -> i32 {
    -1
}

fn cstr_to_str(ptr: *const c_char) -> &'static str {
    if ptr.is_null() { return ""; }
    unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 && len < 4096 { len += 1; }
        core::str::from_utf8(core::slice::from_raw_parts(ptr as *const u8, len)).unwrap_or("")
    }
}

#[no_mangle]
pub extern "C" fn zvfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().open(path_str, flags, pwid)
}

#[no_mangle]
pub extern "C" fn zvfs_close(fd: i32) -> i32 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().close(fd as u32)
}

#[no_mangle]
pub extern "C" fn zvfs_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };
    crate::kernel::fs::zvfs::zvfs::get_zvfs().read(fd as u32, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn zvfs_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };
    crate::kernel::fs::zvfs::zvfs::get_zvfs().write(fd as u32, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn zvfs_mkdir(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().mkdir(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn zvfs_unlink(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().unlink(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn zvfs_rmdir(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().unlink(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn zvfs_stat(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    match crate::kernel::fs::zvfs::zvfs::get_zvfs().stat(path_str, pwid) {
        Some(_) => 0,
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn zvfs_seek(fd: i32, offset: i64, whence: u32) -> i64 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().seek(fd as u32, offset, whence)
}

#[no_mangle]
pub extern "C" fn zvfs_sync() -> i32 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().sync()
}

#[no_mangle]
pub extern "C" fn zvfs_snapshot_create(name: *const c_char) -> i32 {
    let name_str = cstr_to_str(name);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().snapshot_create(name_str)
}

#[no_mangle]
pub extern "C" fn zvfs_snapshot_destroy(snap_id: u64) -> i32 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().snapshot_destroy(snap_id)
}

#[no_mangle]
pub extern "C" fn zvfs_snapshot_rollback(snap_id: u64) -> i32 {
    crate::kernel::fs::zvfs::zvfs::get_zvfs().snapshot_rollback(snap_id)
}

#[no_mangle]
pub extern "C" fn zvfs_clone_create(snap_id: u64, name: *const c_char) -> i32 {
    let name_str = cstr_to_str(name);
    crate::kernel::fs::zvfs::zvfs::get_zvfs().clone_create(snap_id, name_str)
}

#[no_mangle]
pub extern "C" fn zvfs_is_initialized() -> i32 {
    if crate::kernel::fs::zvfs::zvfs::get_zvfs().is_initialized() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn zvfs_get_stats(allocs: *mut u64, frees: *mut u64, reads: *mut u64, writes: *mut u64) {
    let (a, f, r, w) = crate::kernel::fs::zvfs::zvfs::get_zvfs().get_stats();
    unsafe {
        if !allocs.is_null() { *allocs = a; }
        if !frees.is_null() { *frees = f; }
        if !reads.is_null() { *reads = r; }
        if !writes.is_null() { *writes = w; }
    }
}
