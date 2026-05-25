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
pub extern "C" fn hvfs_open(path: *const c_char, flags: u32, pwm: u64) -> i32 {
    let path_str = cstr_to_str(path);
    match crate::kernel::fs::hvfs::hvfs::get_hvfs().open(path_str, flags, pwm) {
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
pub extern "C" fn hvfs_mkdir(path: *const c_char, pwm: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().mkdir(path_str, pwm)
}

#[no_mangle]
pub extern "C" fn hvfs_unlink(path: *const c_char, pwm: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().unlink(path_str, pwm)
}

#[no_mangle]
pub extern "C" fn hvfs_rmdir(path: *const c_char, pwm: u64) -> i32 {
    let path_str = cstr_to_str(path);
    crate::kernel::fs::hvfs::hvfs::get_hvfs().unlink(path_str, pwm)
}

#[no_mangle]
pub extern "C" fn hvfs_stat(path: *const c_char, pwm: u64) -> i32 {
    let path_str = cstr_to_str(path);
    match crate::kernel::fs::hvfs::hvfs::get_hvfs().stat(path_str, pwm) {
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
    // 使用已发现的第一个驱动器, 回退到 drive 0
    let (drive_id, part_start) = hvfs.drives_discovered.lock()
        .first()
        .copied()
        .unwrap_or((hvfs.disk_drive.load(core::sync::atomic::Ordering::Acquire), hvfs.partition_start.load(core::sync::atomic::Ordering::Acquire)));
    hvfs.format_drive(drive_id, part_start);
    0
}

#[no_mangle]
pub extern "C" fn hvfs_disk_init() -> i32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    if !hvfs.is_initialized() { hvfs.init(); }
    0
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_dir(node_id: u32) {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_dir.store(node_id as u64, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_dir() -> u32 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_dir.load(core::sync::atomic::Ordering::Acquire) as u32
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_pwm(pwm: u64) {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_pwm.store(pwm, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_pwm() -> u64 {
    let hvfs = crate::kernel::fs::hvfs::hvfs::get_hvfs();
    hvfs.current_pwm.load(core::sync::atomic::Ordering::Acquire)
}
