use core::ffi::c_char;

use crate::kernel::fs::hvfs::hvfs::get_hvfs;
use super::vfs::VFS_MANAGER;
use crate::kernel::fs::ramfs::ramfs::RAMFS_DATA;
use super::types::*;

const TEST_PWID: u64 = 0x0020F45A8B978417;
static RAMFS_MOUNTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() { return ""; }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

// ============================================================================
// VFS 核心接口 (内部)
// ============================================================================

#[no_mangle]
pub extern "C" fn vfs_init_internal() {
    super::vfs::init();
}

#[no_mangle]
pub extern "C" fn vfs_mount_internal(path: *const c_char, fs_name: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    let fs_name = ptr_to_str(fs_name);

    if fs_name == "ramfs" {
        if !RAMFS_MOUNTED.swap(true, core::sync::atomic::Ordering::SeqCst) {
            let mut ramfs = RAMFS_DATA.lock();
            if ramfs.mount(path) != 0 { return -1; }
        }
    } else if fs_name == "hvfs" {
        let hvfs = get_hvfs();
        if !hvfs.is_initialized() { hvfs.init(); }
    } else {
        return -1;
    }

    VFS_MANAGER.mount(path, fs_name)
}

#[no_mangle]
pub extern "C" fn vfs_unmount_internal(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    VFS_MANAGER.unmount(path)
}

#[no_mangle]
pub extern "C" fn vfs_open_internal(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };

    let mount_idx = match VFS_MANAGER.find_mount(path) {
        Some(idx) => idx, None => return -1,
    };

    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let fs_name: alloc::string::String = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    if fs_name == "ramfs" {
        let fd_idx = match VFS_MANAGER.alloc_fd() { Some(i) => i, None => return -1 };
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.open(rel_path, flags, pwid) {
            Some((inode_num, offset, file_type)) => {
                if (flags & VfsOpenFlags::TRUNC.bits()) != 0 {
                    ramfs.truncate(inode_num, 0, pwid);
                }
                VFS_MANAGER.set_fd(fd_idx, inode_num, offset, flags, pwid, file_type, path);
                fd_idx as i32
            }
            None => {
                if (flags & VfsOpenFlags::CREAT.bits()) != 0 {
                    let (parent_path, name) = if let Some(pos) = rel_path.rfind('/') {
                        if pos == 0 { ("/", &rel_path[1..]) }
                        else { (&rel_path[..pos], &rel_path[pos + 1..]) }
                    } else { ("/", rel_path) };
                    if let Some(new_inode) = ramfs.create_file(parent_path, name, pwid) {
                        let file_type = ramfs.stat(new_inode).map(|s| s.file_type).unwrap_or(0);
                        VFS_MANAGER.set_fd(fd_idx, new_inode, 0, flags, pwid, file_type, path);
                        fd_idx as i32
                    } else { VFS_MANAGER.free_fd(fd_idx); -1 }
                } else { VFS_MANAGER.free_fd(fd_idx); -1 }
            }
        }
    } else if fs_name == "hvfs" {
        let hvfs = get_hvfs();
        let result = hvfs.open(rel_path, flags, pwid);
        if result >= 0 {
            let fd_idx = match VFS_MANAGER.alloc_fd() { Some(i) => i, None => return -1 };
            VFS_MANAGER.set_fd(fd_idx, result as u32, 0, flags, pwid, 0, path);
            fd_idx as i32
        } else {
            result
        }
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_close_internal(fd_idx: u32) -> i32 {
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS { return -1; }
    VFS_MANAGER.free_fd(fd_idx);
    0
}

#[no_mangle]
pub extern "C" fn vfs_read_internal(fd_idx: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 { return -1; }
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS { return -1; }

    let (inode_num, offset, pwid, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_idx < VFS_MAX_FDS && fd_table[fd_idx].used {
            let path = alloc::string::String::from(fd_table[fd_idx].get_path());
            (fd_table[fd_idx].inode_num, fd_table[fd_idx].offset,
             fd_table[fd_idx].pwid, path)
        } else { return -1; }
    };

    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };

    let mount_idx = match VFS_MANAGER.find_mount(&full_path) { Some(i) => i, None => return -1 };

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            let _rel_path = VFS_MANAGER.get_relative_path(&full_path, mount_idx);
            let mut offset = offset;
            let result = ramfs.read(inode_num, &mut offset, buf_slice, pwid);
            VFS_MANAGER.set_fd_offset(fd_idx, offset);
            result
        }
        "hvfs" => {
            let hvfs = get_hvfs();
            hvfs.read(inode_num as u32, buf_slice, count)
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_unlink_internal(path: *const c_char, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };

    let mount_idx = match VFS_MANAGER.find_mount(path) { Some(i) => i, None => return -1 };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => { let mut ramfs = RAMFS_DATA.lock(); ramfs.unlink(rel_path, pwid) }
        "hvfs" => { let hvfs = get_hvfs(); hvfs.unlink(rel_path, pwid) }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_truncate_internal(fd: u32, size: u64) -> i32 {
    let fd_idx = fd as usize;
    if fd_idx >= VFS_MAX_FDS { return -1; }

    let (_inode_num, _offset, pwid, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_idx < VFS_MAX_FDS && fd_table[fd_idx].used {
            let path = alloc::string::String::from(fd_table[fd_idx].get_path());
            (fd_table[fd_idx].inode_num, fd_table[fd_idx].offset,
             fd_table[fd_idx].pwid, path)
        } else { return -1; }
    };

    let mount_idx = match VFS_MANAGER.find_mount(&full_path) { Some(i) => i, None => return -1 };

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.truncate(_inode_num, size, pwid)
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_write_internal(fd_idx: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 { return -1; }
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS { return -1; }

    let (inode_num, offset, pwid, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_idx < VFS_MAX_FDS && fd_table[fd_idx].used {
            let path = alloc::string::String::from(fd_table[fd_idx].get_path());
            (fd_table[fd_idx].inode_num, fd_table[fd_idx].offset,
             fd_table[fd_idx].pwid, path)
        } else { return -1; }
    };

    let buf_slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };

    let mount_idx = match VFS_MANAGER.find_mount(&full_path) { Some(i) => i, None => return -1 };

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            let _rel_path = VFS_MANAGER.get_relative_path(&full_path, mount_idx);
            let mut offset = offset;
            let result = ramfs.write(inode_num, &mut offset, buf_slice, pwid);
            VFS_MANAGER.set_fd_offset(fd_idx, offset);
            result
        }
        "hvfs" => {
            let hvfs = get_hvfs();
            hvfs.write(inode_num as u32, buf_slice, count)
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_mkdir_internal(path: *const c_char, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };

    let mount_idx = match VFS_MANAGER.find_mount(path) { Some(i) => i, None => return -1 };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let (parent_path, name) = if let Some(pos) = rel_path.rfind('/') {
        if pos == 0 { ("/", &rel_path[1..]) }
        else { (&rel_path[..pos], &rel_path[pos + 1..]) }
    } else { ("/", rel_path) };

    if name.is_empty() { return -1; }

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => { let mut ramfs = RAMFS_DATA.lock(); ramfs.mkdir(parent_path, name, pwid) }
        "hvfs" => { let hvfs = get_hvfs(); hvfs.mkdir(rel_path, pwid) }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_rmdir_internal(path: *const c_char, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };

    let mount_idx = match VFS_MANAGER.find_mount(path) { Some(i) => i, None => return -1 };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            match ramfs.resolve_path(rel_path) {
                Some(inode_num) => {
                    let stat = ramfs.stat(inode_num);
                    match stat {
                        Some(s) if s.file_type == VfsFileType::Dir as u8 => ramfs.truncate(inode_num, 0, pwid),
                        _ => -1,
                    }
                }
                None => -1
            }
        }
        "hvfs" => { let hvfs = get_hvfs(); hvfs.unlink(rel_path, pwid) }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_stat_internal(path: *const c_char, st: *mut VfsStat, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };
    if st.is_null() { return -1; }

    let mount_idx = match VFS_MANAGER.find_mount(path) { Some(i) => i, None => return -1 };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    match fs_name.as_str() {
        "ramfs" => {
            let ramfs = RAMFS_DATA.lock();
            match ramfs.resolve_path(rel_path) {
                Some(inode_num) => match ramfs.stat(inode_num) {
                    Some(stat) => { unsafe { *st = stat; } 0 }
                    None => -1
                }
                None => -1
            }
        }
        "hvfs" => {
            let hvfs = get_hvfs();
            match hvfs.stat(rel_path, pwid) {
                Some(obj) => {
                    unsafe {
                        (*st).inode_num = obj.obj_id as u32;
                        (*st).mode = obj.pwid_perm;
                        (*st).size = obj.size as u32;
                        (*st).owner_pwid = obj.owner_pwid;
                        (*st).perm = obj.pwid_perm;
                        (*st).file_type = if obj.is_dir() { 1 } else { 0 };
                    }
                    0
                }
                None => -1
            }
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_readdir_internal(fd: u32, entry: *mut VfsDirent) -> i32 {
    if entry.is_null() { return -1; }

    let fd_idx = fd as usize;
    if fd_idx >= VFS_MAX_FDS { return -1; }

    let (_inode_num, offset, pwid, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_idx < VFS_MAX_FDS && fd_table[fd_idx].used {
            let path = alloc::string::String::from(fd_table[fd_idx].get_path());
            (fd_table[fd_idx].inode_num, fd_table[fd_idx].offset,
             fd_table[fd_idx].pwid, path)
        } else { return -1; }
    };

    let mount_idx = match VFS_MANAGER.find_mount(&full_path) { Some(i) => i, None => return -1 };

    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            alloc::string::String::from(mounts[mount_idx].get_fs_name())
        } else { alloc::string::String::new() }
    };

    let dirent_size = core::mem::size_of::<crate::kernel::fs::ramfs::ramfs::RamFsDirent>() as u64;

    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            let mut dir_offset = offset;
            let mut raw_entry = crate::kernel::fs::ramfs::ramfs::RamFsDirent::new();
            let raw_size = dirent_size as usize;
            let entry_slice = unsafe {
                core::slice::from_raw_parts_mut(
                    &mut raw_entry as *mut crate::kernel::fs::ramfs::ramfs::RamFsDirent as *mut u8,
                    raw_size,
                )
            };
            let result = ramfs.read(_inode_num, &mut dir_offset, entry_slice, pwid);
            if result <= 0 || raw_entry.inode == 0 { return 0; }
            unsafe {
                (*entry).inode = raw_entry.inode;
                (*entry).file_type = raw_entry.file_type;
                let name_len = raw_entry.name.iter().position(|&b| b == 0).unwrap_or(VFS_MAX_NAME);
                let copy_len = name_len.min(VFS_MAX_NAME);
                core::ptr::copy_nonoverlapping(raw_entry.name.as_ptr(), (*entry).name.as_mut_ptr(), copy_len);
                if name_len < VFS_MAX_NAME { (*entry).name[name_len] = 0; }
            }
            VFS_MANAGER.set_fd_offset(fd_idx, dir_offset);
            (raw_entry.inode != 0) as i32
        }
        "hvfs" => -1,  // HvFS v2 readdir via dataset list_entries (TODO)
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn vfs_set_cwd_internal(path: *const c_char) {
    let path = ptr_to_str(path);
    VFS_MANAGER.set_cwd(path);
}

#[no_mangle]
pub extern "C" fn vfs_get_cwd_internal(buf: *mut c_char, size: u32) -> i32 {
    if buf.is_null() || size == 0 { return -1; }
    let cwd = VFS_MANAGER.get_cwd();
    let bytes = cwd.as_bytes();
    let len = bytes.len().min((size - 1) as usize);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
        *buf.add(len) = 0;
    }
    len as i32
}

// ============================================================================
// HvFS v2 直接接口 (internal wrappers)
// ============================================================================

#[no_mangle]
pub extern "C" fn hvfs_init_internal() {
    let hvfs = get_hvfs();
    if !hvfs.is_initialized() { hvfs.init(); }
}

#[no_mangle]
pub extern "C" fn hvfs_format_internal() -> i32 {
    let hvfs = get_hvfs();
    hvfs.format_disk();
    0
}

#[no_mangle]
pub extern "C" fn hvfs_check_disk_internal() -> i32 {
    let hvfs = get_hvfs();
    hvfs.is_disk_mode() as i32
}

#[no_mangle]
pub extern "C" fn hvfs_set_disk_present_internal(present: bool) {
    let hvfs = get_hvfs();
    if present { hvfs.spa.disk_present.store(true, core::sync::atomic::Ordering::Release); }
}

#[no_mangle]
pub extern "C" fn hvfs_open_internal(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };
    let hvfs = get_hvfs();
    hvfs.open(path, flags, pwid)
}

#[no_mangle]
pub extern "C" fn hvfs_close_internal(fd: u32) -> i32 {
    let hvfs = get_hvfs();
    hvfs.close(fd)
}

#[no_mangle]
pub extern "C" fn hvfs_read_internal(fd: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };
    let hvfs = get_hvfs();
    hvfs.read(fd, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn hvfs_write_internal(fd: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 { return -1; }
    let buf_slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };
    let hvfs = get_hvfs();
    hvfs.write(fd, buf_slice, count)
}

#[no_mangle]
pub extern "C" fn hvfs_mkdir_internal(path: *const c_char, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwid = if pwid == 0 { TEST_PWID } else { pwid };
    let hvfs = get_hvfs();
    hvfs.mkdir(path, pwid)
}

#[no_mangle]
pub extern "C" fn hvfs_sync_internal() -> i32 {
    let hvfs = get_hvfs();
    hvfs.sync()
}

#[no_mangle]
pub extern "C" fn hvfs_get_stats_internal(total_blocks: *mut u32, free_blocks: *mut u32,
                                           total_inodes: *mut u32, free_inodes: *mut u32) {
    let hvfs = get_hvfs();
    let (allocs, frees, _reads, _writes) = hvfs.get_stats();
    unsafe {
        if !total_blocks.is_null() { *total_blocks = allocs as u32; }
        if !free_blocks.is_null() { *free_blocks = frees as u32; }
        if !total_inodes.is_null() { *total_inodes = 0; }
        if !free_inodes.is_null() { *free_inodes = 0; }
    }
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_dir_internal(_inode_num: u32) {
    let hvfs = get_hvfs();
    hvfs.current_dir.store(_inode_num as u64, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_dir_internal() -> u32 {
    let hvfs = get_hvfs();
    hvfs.current_dir.load(core::sync::atomic::Ordering::Acquire) as u32
}

#[no_mangle]
pub extern "C" fn hvfs_set_current_pwid_internal(pwid: u64) {
    let hvfs = get_hvfs();
    hvfs.current_pwid.store(pwid, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub extern "C" fn hvfs_get_current_pwid_internal() -> u64 {
    let hvfs = get_hvfs();
    hvfs.current_pwid.load(core::sync::atomic::Ordering::Acquire)
}

// ============================================================================
// Barrier 接口
// ============================================================================

#[no_mangle]
pub extern "C" fn vfs_barrier_capture() {
    VFS_MANAGER.capture_snapshot();
}

#[no_mangle]
pub extern "C" fn vfs_barrier_restore() -> i32 {
    VFS_MANAGER.restore_from_snapshot();
    1
}

// ============================================================================
// 公共 VFS API
// ============================================================================

#[no_mangle]
pub extern "C" fn vfs_init() { vfs_init_internal(); }

#[no_mangle]
pub extern "C" fn vfs_mount(path: *const c_char, fs_name: *const c_char) -> i32 {
    vfs_mount_internal(path, fs_name)
}

#[no_mangle]
pub extern "C" fn vfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    vfs_open_internal(path, flags, pwid)
}

#[no_mangle]
pub extern "C" fn vfs_close(fd: u32) -> i32 { vfs_close_internal(fd) }

#[no_mangle]
pub extern "C" fn vfs_read(fd: u32, buf: *mut u8, count: u32) -> i32 {
    vfs_read_internal(fd, buf, count)
}

#[no_mangle]
pub extern "C" fn vfs_write(fd: u32, buf: *const u8, count: u32) -> i32 {
    vfs_write_internal(fd, buf, count)
}

#[no_mangle]
pub extern "C" fn vfs_stat(path: *const c_char, st: *mut VfsStat, pwid: u64) -> i32 {
    vfs_stat_internal(path, st, pwid)
}

#[no_mangle]
pub extern "C" fn vfs_mkdir(path: *const c_char, pwid: u64) -> i32 {
    vfs_mkdir_internal(path, pwid)
}

#[no_mangle]
pub extern "C" fn vfs_chmod(_path: *const c_char, _mode: u16, _pwid: u64) -> i32 { 0 }

#[no_mangle]
pub extern "C" fn vfs_chown(_path: *const c_char, _owner_pwid: u64, _pwid: u64) -> i32 { 0 }

#[no_mangle]
pub extern "C" fn vfs_unlink(path: *const c_char, pwid: u64) -> i32 {
    vfs_unlink_internal(path, pwid)
}

#[no_mangle]
pub extern "C" fn vfs_rename(_old: *const c_char, _new: *const c_char, _pwid: u64) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn vfs_rmdir(path: *const c_char, pwid: u64) -> i32 {
    vfs_rmdir_internal(path, pwid)
}

#[no_mangle]
pub extern "C" fn vfs_readdir(fd: u32, entry: *mut VfsDirent) -> i32 {
    vfs_readdir_internal(fd, entry)
}

#[no_mangle]
pub extern "C" fn vfs_sync() -> i32 {
    hvfs_sync_internal()
}

#[no_mangle]
pub extern "C" fn vfs_get_cwd(buf: *mut c_char, size: u32) -> i32 {
    vfs_get_cwd_internal(buf, size)
}

#[no_mangle]
pub extern "C" fn vfs_set_cwd(path: *const c_char) {
    vfs_set_cwd_internal(path);
}

#[no_mangle]
pub extern "C" fn vfs_seek(fd: u32, offset: i32, whence: u32) -> i32 {
    let hvfs = get_hvfs();
    hvfs.seek(fd, offset as i64, whence) as i32
}

#[no_mangle]
pub extern "C" fn vfs_fd_table() -> *const core::ffi::c_void {
    VFS_MANAGER.fd_table.lock().as_ptr() as *const core::ffi::c_void
}

#[no_mangle]
pub extern "C" fn vfs_format_internal(path: *const c_char, fs_type: *const c_char) -> i32 {
    let _fs_type = ptr_to_str(fs_type);
    let _path = ptr_to_str(path);
    -1
}
