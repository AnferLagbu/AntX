use core::ffi::c_char;

#[no_mangle]
pub extern "C" fn hvfs_init() -> i32 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().init();
    0
}

#[no_mangle]
pub extern "C" fn hvfs_mount(_path: *const c_char) -> i32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    if hvfs.is_initialized() { 0 } else { -1 }
}

#[no_mangle]
pub extern "C" fn hvfs_unmount() -> i32 {
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
pub extern "C" fn hvfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    match crate::kernel::fs::hvfs::hvfs::get_hvfs().open(path_str, flags, pwid) {
        Ok(fd) => fd,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub extern "C" fn hvfs_close(fd: i32) -> i32 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().close(fd as u32)
}

#[no_mangle]
pub extern "C" fn hvfs_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };
    crate::kernel::fs::hvfs::hvfs::get_hvfs().read(fd as u32, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn hvfs_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };
    crate::kernel::fs::hvfs::hvfs::get_hvfs().write(fd as u32, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn hvfs_mkdir(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().mkdir(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn hvfs_unlink(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().unlink(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn hvfs_rmdir(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().unlink(path_str, pwid)
}

#[no_mangle]
pub extern "C" fn hvfs_stat(path: *const c_char, pwid: u64) -> i32 {
    let path_str = cstr_to_str(path);
    match crate::kernel::fs::hvfs::hvfs::get_hvfs().stat(path_str, pwid) {
        Some(_) => 0,
        None => -1,
    }
}

#[no_mangle]
pub extern "C" fn hvfs_seek(fd: i32, offset: i64, whence: u32) -> i64 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().seek(fd as u32, offset, whence)
}

#[no_mangle]
pub extern "C" fn hvfs_sync() -> i32 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().sync()
}

#[no_mangle]
pub extern "C" fn hvfs_snapshot_create(name: *const c_char) -> i32 {
    let name_str = cstr_to_str(name);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().snapshot_create(name_str)
}

#[no_mangle]
pub extern "C" fn hvfs_snapshot_destroy(snap_id: u64) -> i32 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().snapshot_destroy(snap_id)
}

#[no_mangle]
pub extern "C" fn hvfs_snapshot_rollback(snap_id: u64) -> i32 {
    crate::kernel::fs::hvfs::hvfs::get_hvfs().snapshot_rollback(snap_id)
}

#[no_mangle]
pub extern "C" fn hvfs_clone_create(snap_id: u64, name: *const c_char) -> i32 {
    let name_str = cstr_to_str(name);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().clone_create(snap_id, name_str)
}

#[no_mangle]
pub extern "C" fn hvfs_is_initialized() -> i32 {
    if crate::kernel::fs::hvfs::hvfs::get_hvfs().is_initialized() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn hvfs_get_stats(allocs: *mut u64, frees: *mut u64, reads: *mut u64, writes: *mut u64) {
    let (a, f, r, w) = crate::kernel::fs::hvfs::hvfs::get_hvfs().get_stats();
    unsafe {
        if !allocs.is_null() { *allocs = a; }
        if !frees.is_null() { *frees = f; }
        if !reads.is_null() { *reads = r; }
        if !writes.is_null() { *writes = w; }
    }
}

#[no_mangle]
pub extern "C" fn hvfs_format() -> i32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.format_disk();
    0
}

#[no_mangle]
pub extern "C" fn hvfs_disk_init() -> i32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    if !hvfs.is_initialized() { hvfs.init(); }
    0
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_dir(inode_num: u32) {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_dir.store(inode_num as u64, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_dir() -> u32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_dir.load(core::sync::atomic::Ordering::Acquire) as u32
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_pwid(pwid: u64) {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_pwid.store(pwid, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_pwid() -> u64 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_pwid.load(core::sync::atomic::Ordering::Acquire)
}
