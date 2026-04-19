use alloc::string::String;
use core::ffi::c_char;

use super::vfs::VFS_MANAGER;
use super::types::{VFS_MAX_MOUNTS, VFS_MAX_FDS};
use super::ramfs::RAMFS_DATA;
use super::hvfs::{get_hvfs, HVFS_DATA};
use super::diskfs::{get_diskfs, DISKFS_DATA};
use super::types::*;

fn ptr_to_str<'a>(ptr: *const c_char) -> &'a str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let len = (0..).find(|&i| *ptr.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(ptr as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_init() {
    super::vfs::init();
    super::ramfs::init();
    super::hvfs::init();
    super::diskfs::init();
}

#[no_mangle]
pub extern "C" fn rust_vfs_mount(path: *const c_char, fs_name: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    let fs_name = ptr_to_str(fs_name);
    
    if fs_name == "ramfs" {
        let mut ramfs = RAMFS_DATA.lock();
        if ramfs.mount(path) != 0 {
            return -1;
        }
    } else if fs_name == "diskfs" {
        let mut diskfs = get_diskfs().lock();
        if diskfs.mount(path) != 0 {
            return -1;
        }
    } else {
        return -1;
    }
    
    VFS_MANAGER.mount(path, fs_name)
}

#[no_mangle]
pub extern "C" fn rust_vfs_unmount(path: *const c_char) -> i32 {
    let path = ptr_to_str(path);
    VFS_MANAGER.unmount(path)
}

#[no_mangle]
pub extern "C" fn rust_vfs_open(path: *const c_char, flags: u32, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    
    let mount_idx = match VFS_MANAGER.find_mount(path) {
        Some(idx) => idx,
        None => return -1,
    };
    
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);
    
    let fd_idx = match VFS_MANAGER.alloc_fd() {
        Some(idx) => idx,
        None => return -1,
    };
    
    let fs_name: String = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            String::from(mounts[mount_idx].get_fs_name())
        } else {
            String::new()
        }
    };
    
    if fs_name == "ramfs" {
        let mut ramfs = RAMFS_DATA.lock();
        match ramfs.open(rel_path, flags, pwid) {
            Some((inode_num, offset, file_type)) => {
                VFS_MANAGER.set_fd(fd_idx, inode_num, offset, flags, pwid, file_type, rel_path);
                fd_idx as i32
            }
            None => {
                VFS_MANAGER.free_fd(fd_idx);
                -1
            }
        }
    } else if fs_name == "diskfs" {
        let mut diskfs = get_diskfs().lock();
        match diskfs.open(rel_path, flags, pwid) {
            Some((inode_num, offset, file_type)) => {
                VFS_MANAGER.set_fd(fd_idx, inode_num, offset, flags, pwid, file_type, rel_path);
                fd_idx as i32
            }
            None => {
                VFS_MANAGER.free_fd(fd_idx);
                -1
            }
        }
    } else {
        VFS_MANAGER.free_fd(fd_idx);
        -1
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_close(fd_idx: u32) -> i32 {
    let fd_idx = fd_idx as usize;
    
    if fd_idx >= VFS_MAX_FDS {
        return -1;
    }
    
    VFS_MANAGER.free_fd(fd_idx);
    0
}

#[no_mangle]
pub extern "C" fn rust_vfs_read(fd_idx: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }
    
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS {
        return -1;
    }
    
    let (inode_num, offset, pwid) = match VFS_MANAGER.get_fd_info(fd_idx) {
        Some(info) => info,
        None => return -1,
    };
    
    let buf_slice = unsafe { core::slice::from_raw_parts_mut(buf, count as usize) };
    
    let (fs_type, path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        let path_str = fd_table[fd_idx].get_path();
        (fd_table[fd_idx].file_type, String::from(path_str))
    };
    
    let mount_idx = match VFS_MANAGER.find_mount(&path) {
        Some(idx) => idx,
        None => return -1,
    };
    
    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            String::from(mounts[mount_idx].get_fs_name())
        } else {
            String::new()
        }
    };
    
    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            let rel_path = VFS_MANAGER.get_relative_path(&path, mount_idx);
            let mut offset = offset;
            let result = ramfs.read(inode_num, &mut offset, buf_slice, pwid);
            VFS_MANAGER.set_fd_offset(fd_idx, offset);
            result
        }
        "diskfs" => {
            let mut diskfs = get_diskfs().lock();
            let rel_path = VFS_MANAGER.get_relative_path(&path, mount_idx);
            let result = diskfs.read(inode_num, buf_slice, count);
            result
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_write(fd_idx: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }
    
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS {
        return -1;
    }
    
    let (inode_num, offset, pwid) = match VFS_MANAGER.get_fd_info(fd_idx) {
        Some(info) => info,
        None => return -1,
    };
    
    let buf_slice = unsafe { core::slice::from_raw_parts(buf, count as usize) };
    
    let path = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        String::from(fd_table[fd_idx].get_path())
    };
    
    let mount_idx = match VFS_MANAGER.find_mount(&path) {
        Some(idx) => idx,
        None => return -1,
    };
    
    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            String::from(mounts[mount_idx].get_fs_name())
        } else {
            String::new()
        }
    };
    
    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            let rel_path = VFS_MANAGER.get_relative_path(&path, mount_idx);
            let mut offset = offset;
            let result = ramfs.write(inode_num, &mut offset, buf_slice, pwid);
            VFS_MANAGER.set_fd_offset(fd_idx, offset);
            result
        }
        "diskfs" => {
            let mut diskfs = get_diskfs().lock();
            let rel_path = VFS_MANAGER.get_relative_path(&path, mount_idx);
            let result = diskfs.write(inode_num, buf_slice, count);
            result
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_mkdir(path: *const c_char, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    
    let mount_idx = match VFS_MANAGER.find_mount(path) {
        Some(idx) => idx,
        None => return -1,
    };
    
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);
    
    let (parent_path, name) = if let Some(pos) = rel_path.rfind('/') {
        if pos == 0 {
            ("/", &rel_path[1..])
        } else {
            (&rel_path[..pos], &rel_path[pos + 1..])
        }
    } else {
        ("/", rel_path)
    };
    
    if name.is_empty() {
        return -1;
    }
    
    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            String::from(mounts[mount_idx].get_fs_name())
        } else {
            String::new()
        }
    };
    
    match fs_name.as_str() {
        "ramfs" => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.mkdir(parent_path, name, pwid)
        }
        "diskfs" => {
            let mut diskfs = get_diskfs().lock();
            diskfs.mkdir(parent_path, name, pwid)
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_stat(path: *const c_char, st: *mut VfsStat, pwid: u64) -> i32 {
    let path = ptr_to_str(path);
    
    if st.is_null() {
        return -1;
    }
    
    let mount_idx = match VFS_MANAGER.find_mount(path) {
        Some(idx) => idx,
        None => return -1,
    };
    
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);
    
    let fs_name = {
        let mounts = VFS_MANAGER.mounts.lock();
        if mount_idx < VFS_MAX_MOUNTS {
            String::from(mounts[mount_idx].get_fs_name())
        } else {
            String::new()
        }
    };
    
    match fs_name.as_str() {
        "ramfs" => {
            let ramfs = RAMFS_DATA.lock();
            match ramfs.resolve_path(rel_path) {
                Some(inode_num) => {
                    match ramfs.stat(inode_num) {
                        Some(stat) => { unsafe { *st = stat; } 0 }
                        None => -1
                    }
                }
                None => -1
            }
        }
        "diskfs" => {
            let diskfs = get_diskfs().lock();
            match diskfs.stat(rel_path, pwid) {
                Some(stat) => { unsafe { *st = stat; } 0 }
                None => -1
            }
        }
        _ => -1
    }
}

#[no_mangle]
pub extern "C" fn rust_vfs_set_cwd(path: *const c_char) {
    let path = ptr_to_str(path);
    VFS_MANAGER.set_cwd(path);
}

#[no_mangle]
pub extern "C" fn rust_vfs_get_cwd(buf: *mut c_char, size: u32) -> i32 {
    if buf.is_null() || size == 0 {
        return -1;
    }
    
    let cwd = VFS_MANAGER.get_cwd();
    let bytes = cwd.as_bytes();
    let len = bytes.len().min((size - 1) as usize);
    
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, len);
        *buf.add(len) = 0;
    }
    
    len as i32
}

#[no_mangle]
pub extern "C" fn rust_hvfs_init() {
    super::hvfs::init();
}

#[no_mangle]
pub extern "C" fn rust_hvfs_format() -> i32 {
    let mut hvfs = get_hvfs().lock();
    hvfs.format()
}

#[no_mangle]
pub extern "C" fn rust_hvfs_check_disk() -> i32 {
    let mut hvfs = get_hvfs().lock();
    hvfs.check_disk()
}

#[no_mangle]
pub extern "C" fn rust_hvfs_set_disk_present(present: bool) {
    let mut hvfs = get_hvfs().lock();
    hvfs.set_disk_present(present);
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
