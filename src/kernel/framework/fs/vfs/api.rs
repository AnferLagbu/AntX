//! VFS 对外 API (syscall 边界)
//!
//! ## 调用方契约
//! - `syscall::sys_read/write/open/close` —— 用户态文件操作
//! - `proc::exec::load_elf` —— 加载 ELF 时通过 VFS 读文件
//! - `credo::storage` —— 持久化身份数据
//! - `host-tests` —— host 端单元测试
//!
//! ## 内部接口
//! - `vfs_*_internal` 是核心实现,**不对外**;`#[no_mangle]` 的 `vfs_*`
//!   函数将指针参数 (来自用户态/asm) 转为 `&str` 后委托给内部实现。
//!
//! ## 安全约束
//! - 所有 `*_internal` 函数在调用前已验证指针非空与字符串 UTF-8
//!   (委托给 [`CStrExt::as_kstr`])。
//! - `&[u8]` buffer 长度由调用方提供,实现按需截断,不会越界写。
//!
//! ## 性能特征
//! - 静态分发,无 vtable 开销
//! - 字符串路径解析纯栈上,无堆分配 (除路径 split 时 alloc::string)
use super::types::*;
use super::vfs::VFS_MANAGER;
use crate::kernel::framework::fs::hvfs::hvfs::get_hvfs;
use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
use crate::kernel::framework::mm::{pcache, PAGE_SIZE};
use crate::kernel::framework::userptr::{UserReadPtr, UserWritePtr, UserRefMut};
use crate::kernel::framework::lib::cstr::CStrExt;
use crate::kernel::framework::syscall::epoll as fw_epoll;

/// B2: 4KB 对齐 read 时的 pcache 命中快路径上限 (16 页 = 64KB)
const PCACHE_FAST_MAX_BYTES: usize = 64 * 1024;
/// B2: 4KB 对齐 read 时的 pcache 命中快路径下限 (1 页 = 4KB)
const PCACHE_FAST_MIN_BYTES: usize = PAGE_SIZE as usize;

// ============================================================================
// 对外契约: Vfs trait (用于 trait-object 注册 / host 端测试)
// ============================================================================
//
// 注: 此 trait 是 **声明性契约**,不替换现有 #[no_mangle] 函数。内部
// `vfs_*_internal` 仍是真实入口;`Vfs` trait 为未来 hot-swap / mock 测试
// 预留接口边界,impl 见 fs::ramfs/ramfs::RamFs 等。
pub trait Vfs: Send + Sync {
    fn name(&self) -> &'static str;
    fn mount(&self, path: &str) -> KernelResult<()>;
    fn unmount(&self) -> KernelResult<()>;
}

const TEST_PWM: u64 = 0x0020F45A8B978417;
static RAMFS_MOUNTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// 兼容旧 `ptr_to_str(ptr)` 调用语义:
/// - 空指针 → `""`
/// - 非 UTF-8 → `""`(降级)
/// - 超过 `VFS_MAX_PATH` 长度 → 截断到该上限
///
/// 委托给统一抽象 [`CStrExt::as_kstr`],行为完全一致。
fn ptr_to_str<'a>(ptr: *const u8) -> &'a str {
    ptr.as_kstr()
}

fn resolve_pwm(pwm: u64) -> u64 {
    if pwm == 0 {
        TEST_PWM
    } else {
        pwm
    }
}

fn get_fd_info(fd_idx: u32) -> Option<(u32, u64, u64, alloc::string::String)> {
    let fd_idx = fd_idx as usize;
    if fd_idx >= VFS_MAX_FDS {
        return None;
    }
    let fd_table = VFS_MANAGER.fd_table.lock();
    if fd_idx < VFS_MAX_FDS && fd_table[fd_idx].used {
        let path = alloc::string::String::from(fd_table[fd_idx].get_path());
        Some((
            fd_table[fd_idx].node_id,
            fd_table[fd_idx].offset,
            fd_table[fd_idx].pwm,
            path,
        ))
    } else {
        None
    }
}

fn split_parent_name(rel_path: &str) -> (&str, &str) {
    if let Some(pos) = rel_path.rfind('/') {
        if pos == 0 {
            ("/", &rel_path[1..])
        } else {
            (&rel_path[..pos], &rel_path[pos + 1..])
        }
    } else {
        ("/", rel_path)
    }
}

// ============================================================================
// VFS 核心接口 (内部)
// ============================================================================

#[no_mangle]
pub fn vfs_init_internal() {
    super::vfs::init();
}

#[no_mangle]
pub fn vfs_mount_internal(path: *const u8, fs_name: *const u8) -> i32 {
    let path = ptr_to_str(path);
    let fs_name = ptr_to_str(fs_name);
    let fs_type = FsType::from_name(fs_name);

    match fs_type {
        FsType::RamFs => {
            if !RAMFS_MOUNTED.swap(true, core::sync::atomic::Ordering::SeqCst) {
                let mut ramfs = RAMFS_DATA.lock();
                if ramfs.mount(path) != 0 {
                    return -1;
                }
            }
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            if !hvfs.is_initialized() {
                hvfs.init();
            }
        }

        FsType::Unknown => return -1,
    }

    VFS_MANAGER.mount(path, fs_name).as_i32()
}

#[no_mangle]
pub fn vfs_unmount_internal(path: *const u8) -> i32 {
    let path = ptr_to_str(path);
    VFS_MANAGER.unmount(path).as_i32()
}

#[no_mangle]
pub fn vfs_open_internal(path: *const u8, flags: u32, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    match fs_type {
        FsType::RamFs => {
            let fd_idx = match VFS_MANAGER.alloc_fd() {
                Some(i) => i,
                None => return -1,
            };
            let mut ramfs = RAMFS_DATA.lock();
            match ramfs.open(rel_path, flags, pwm) {
                Some((node_id, offset, file_type)) => {
                    if (flags & VfsOpenFlags::TRUNC.bits()) != 0 {
                        ramfs.truncate(node_id, 0, pwm);
                    }
                    VFS_MANAGER.set_fd(fd_idx, node_id, offset, flags, pwm, file_type, path);
                    fd_idx as i32
                }
                None => {
                    if (flags & VfsOpenFlags::CREAT.bits()) != 0 {
                        let (parent_path, name) = split_parent_name(rel_path);
                        if let Some(new_inode) = ramfs.create_file(parent_path, name, pwm) {
                            let file_type = ramfs.stat(new_inode).map(|s| s.file_type).unwrap_or(0);
                            VFS_MANAGER.set_fd(fd_idx, new_inode, 0, flags, pwm, file_type, path);
                            fd_idx as i32
                        } else {
                            VFS_MANAGER.free_fd(fd_idx);
                            -1
                        }
                    } else {
                        VFS_MANAGER.free_fd(fd_idx);
                        -1
                    }
                }
            }
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            match hvfs.open(rel_path, flags, pwm) {
                Ok(hvfs_fd) => {
                    let fd_idx = match VFS_MANAGER.alloc_fd() {
                        Some(i) => i,
                        None => return -1,
                    };
                    VFS_MANAGER.set_fd(fd_idx, hvfs_fd as u32, 0, flags, pwm, 0, path);
                    fd_idx as i32
                }
                Err(e) => e.as_i32(),
            }
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_close_internal(fd_idx: u32) -> i32 {
    let fd_idx_us = fd_idx as usize;
    if fd_idx_us >= VFS_MAX_FDS {
        return -1;
    }
    // B2: 释放该 fd 关联 inode 的全部 pcache 缓存页, 避免内存泄漏
    if let Some((node_id, _, _, _)) = get_fd_info(fd_idx) {
        pcache::pcache_invalidate_inode(node_id);
    }
    VFS_MANAGER.free_fd(fd_idx_us);
    // C1: fd 关闭 → 唤醒该 fd 注册的所有 epoll 等待者 (EPOLLHUP|EPOLLERR)
    fw_epoll::epoll_pwake(fd_idx as i32);
    0
}

#[no_mangle]
pub fn vfs_read_internal(fd_idx: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let (node_id, offset, pwm, full_path) = match get_fd_info(fd_idx) {
        Some(info) => info,
        None => return -1,
    };

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut user_buf = unsafe { UserWritePtr::new(buf, count as usize) };

    let (_, fs_type) = match VFS_MANAGER.resolve_mount(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    match fs_type {
        FsType::RamFs => {
            // B2: 4KB 对齐 + 全部 pcache 命中 → 走 pcache 快路径
            // 条件: count ∈ [1, 16] 页 且 offset / count 均为 4KB 对齐
            let is_aligned_4k = (count as u64) >= PCACHE_FAST_MIN_BYTES as u64
                && (count as u64) <= PCACHE_FAST_MAX_BYTES as u64
                && (count as u64) % PAGE_SIZE == 0
                && (offset as u64) % PAGE_SIZE == 0;

            if is_aligned_4k {
                let npages = (count as u64 / PAGE_SIZE) as usize;
                let first_pi = offset / PAGE_SIZE;

                // 步骤1: 探测全部页是否在 pcache 中
                let mut all_hit = true;
                for i in 0..npages {
                    if pcache::pcache_lookup(node_id, first_pi + i as u64).is_none() {
                        all_hit = false;
                        break;
                    }
                }

                if all_hit {
                    // 步骤2: 全部命中, 直接从 pcache 复制到用户 buf
                    let mut all_ok = true;
                    for i in 0..npages {
                        // SAFETY: 4KB 对齐保证 buf.add(i*PAGE_SIZE) 落在 [buf, buf+count) 内
                        let dst = unsafe {
                            core::slice::from_raw_parts_mut(
                                buf.add(i * PAGE_SIZE as usize),
                                PAGE_SIZE as usize,
                            )
                        };
                        if !pcache::pcache_read_to_slice(node_id, first_pi + i as u64, dst) {
                            all_ok = false;
                            break;
                        }
                    }
                    if all_ok {
                        VFS_MANAGER.set_fd_offset(fd_idx as usize, offset + count as u64);
                        return count as i32;
                    }
                }
            }

            // 慢速路径: 原 ramfs.read (不填 pcache; pcache 由 mmap 路径 / 显式预热填)
            let mut ramfs = RAMFS_DATA.lock();
            let mut new_offset = offset;
            let result = ramfs.read(node_id, &mut new_offset, user_buf.as_mut_slice(), pwm);
            VFS_MANAGER.set_fd_offset(fd_idx as usize, new_offset);
            result
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.read(node_id, user_buf.as_mut_slice(), count)
        }

        FsType::Unknown => -1,
    }
}

/// 按 inode_id 直接读取文件数据 (B2: mmap prewarm 用)
///
/// 区别于 `vfs_read_internal`: 不依赖 fd, 而是按 inode 寻址.
/// 用于 mmap 创建 VMA 时, 同步预热 Page Cache (prewarm 全部页).
///
/// 参数:
/// - `node_id`: ramfs 内部 inode 编号
/// - `file_offset`: 文件内字节偏移 (调用方保证页对齐)
/// - `dst`: 目标缓冲区 (长度由调用方提供, 通常为 PAGE_SIZE)
/// - `pwm`: 权限字 (0 时使用 TEST_PWM)
///
/// 返回: 实际读取字节数, 负数表示错误.
#[no_mangle]
pub fn vfs_pread_inode(node_id: u32, file_offset: u64, dst: &mut [u8], pwm: u64) -> i32 {
    let pwm = resolve_pwm(pwm);
    // SAFETY: 调用方保证 dst 在生命周期内有效; 长度由调用方控制.
    let mut user_buf = unsafe { UserWritePtr::new(dst.as_mut_ptr(), dst.len()) };

    // B2: 当前 mmap prewarm 仅支持 RamFs (HvFs 后续集成)
    let mut ramfs = RAMFS_DATA.lock();
    let mut offset = file_offset;
    ramfs.read(node_id, &mut offset, user_buf.as_mut_slice(), pwm)
}

#[no_mangle]
pub fn vfs_unlink_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.unlink(rel_path, pwm)
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.unlink(rel_path, pwm)
        }

        FsType::Unknown => -1,
    }
}
// ============================================================================
// link / symlink / readlink — 见 services/fs/link.rs, 在 ramfs/hvfs
// 真正实现 link/symlink 前, 暂时由 dispatch 直接返回 ENOSYS.
// 保留 framework API 的需求: 一旦 ramfs/hvfs 支持, services 不变, 仅
// 调整 framework 实现即可. 当前未保留 stub, 避免假实现.
// ============================================================================

#[no_mangle]
pub fn vfs_truncate_internal(fd: u32, size: u64) -> i32 {
    let fd_idx = fd as usize;
    if fd_idx >= VFS_MAX_FDS {
        return -1;
    }

    let (_node_id, _offset, pwm, full_path) = match get_fd_info(fd) {
        Some(info) => info,
        None => return -1,
    };

    let (_, fs_type) = match VFS_MANAGER.resolve_mount(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.truncate(_node_id, size, pwm)
        }
        FsType::HvFs | FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_write_internal(fd_idx: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let (node_id, offset, pwm, full_path) = match get_fd_info(fd_idx) {
        Some(info) => info,
        None => return -1,
    };

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let user_buf = unsafe { UserReadPtr::new(buf, count as usize) };

    let (_, fs_type) = match VFS_MANAGER.resolve_mount(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            let mut offset = offset;
            let result = ramfs.write(node_id, &mut offset, user_buf.as_slice(), pwm);
            VFS_MANAGER.set_fd_offset(fd_idx as usize, offset);
            // C1: 写完成 → 唤醒该 fd 注册的所有 epoll 等待者 (EPOLLOUT)
            fw_epoll::epoll_pwake(fd_idx as i32);
            result
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.write(node_id, user_buf.as_slice(), count)
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_mkdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let (parent_path, name) = split_parent_name(rel_path);
    if name.is_empty() {
        return -1;
    }

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.mkdir(parent_path, name, pwm)
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.mkdir(rel_path, pwm)
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_rmdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            match ramfs.resolve_path(rel_path) {
                Some(node_id) => {
                    let stat = ramfs.stat(node_id);
                    match stat {
                        Some(s) if s.file_type == VfsFileType::Dir.as_u8() => {
                            ramfs.truncate(node_id, 0, pwm)
                        }
                        _ => -1,
                    }
                }
                None => -1,
            }
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.unlink(rel_path, pwm)
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_stat_internal(path: *const u8, st: *mut VfsStat, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let _pwm = resolve_pwm(pwm);
    if st.is_null() {
        return -1;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_ref = unsafe { UserRefMut::new(st) };

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let result = match fs_type {
        FsType::RamFs => {
            let ramfs = RAMFS_DATA.lock();
            match ramfs.resolve_path(rel_path) {
                Some(node_id) => match ramfs.stat(node_id) {
                    Some(stat) => {
                        *st_ref.as_mut() = stat;
                        0
                    }
                    None => -1,
                },
                None => -1,
            }
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            match hvfs.stat(rel_path, pwm) {
                Some(obj) => {
                    let r = st_ref.as_mut();
                    r.node_id = obj.obj_id as u32;
                    r.mode = obj.pwm_perm;
                    r.size = obj.size as u32;
                    r.owner_pwm = obj.owner_pwm;
                    r.group_pwm = obj.group_pwm;
                    r.perm = obj.pwm_perm;
                    r.sensitivity = obj.sensitivity;
                    r.file_type = if obj.is_dir() {
                        VfsFileType::Dir.as_u8()
                    } else {
                        VfsFileType::File.as_u8()
                    };
                    0
                }
                None => -1,
            }
        }

        FsType::Unknown => -1,
    };

    if result == 0 {
        let tbl = crate::kernel::framework::credo::identity::get_table();
        let r = st_ref.as_mut();
        r.uid = tbl.uid_of(r.owner_pwm);
        r.gid = tbl.gid_of(r.group_pwm);
        if r.gid == 0xFFFF_FFFF {
            r.gid = r.uid;
        }
    }

    result
}

#[no_mangle]
pub fn vfs_readdir_internal(fd: u32, entry: *mut VfsDirEntry) -> i32 {
    if entry.is_null() {
        return -1;
    }

    let (_node_id, offset, pwm, full_path) = match get_fd_info(fd) {
        Some(info) => info,
        None => return -1,
    };

    let (_, fs_type) = match VFS_MANAGER.resolve_mount(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    let dirent_size = core::mem::size_of::<crate::kernel::framework::fs::ramfs::ramfs::RamFsDirEntry>() as u64;

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            let mut dir_offset = offset;
            let raw_size = dirent_size as usize;
            let mut raw_buf = alloc::vec![0u8; raw_size];
            let result = ramfs.read(_node_id, &mut dir_offset, &mut raw_buf, pwm);
            let raw_entry = crate::kernel::framework::fs::ramfs::ramfs::RamFsDirEntry::read_at(&raw_buf, 0);
            if result <= 0 || raw_entry.node == 0 {
                return 0;
            }
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let mut entry_ref = unsafe { UserRefMut::new(entry) };
            let e = entry_ref.as_mut();
            e.node = raw_entry.node;
            e.file_type = raw_entry.file_type;
            let name_len = raw_entry
                .name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(VFS_MAX_NAME);
            let copy_len = name_len.min(VFS_MAX_NAME);
            e.name[..copy_len].copy_from_slice(&raw_entry.name[..copy_len]);
            if name_len < VFS_MAX_NAME {
                e.name[name_len] = 0;
            }
            VFS_MANAGER.set_fd_offset(fd as usize, dir_offset);
            (raw_entry.node != 0) as i32
        }
        FsType::HvFs => -1,
        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_set_cwd_internal(path: *const u8) {
    let path = ptr_to_str(path);
    VFS_MANAGER.set_cwd(path);
}

#[no_mangle]
pub fn vfs_get_cwd_internal(buf: *mut u8, size: u32) -> i32 {
    if buf.is_null() || size == 0 {
        return -1;
    }
    let cwd = VFS_MANAGER.get_cwd();
    let bytes = cwd.as_bytes();
    let len = bytes.len().min((size - 1) as usize);
    // SAFETY: `mut` 由调用方保证为有效指针; 只读访问
    let mut user_buf = unsafe { UserWritePtr::new(buf as *mut u8, size as usize) };
    let slice = user_buf.as_mut_slice();
    slice[..len].copy_from_slice(&bytes[..len]);
    slice[len] = 0;
    len as i32
}

// ============================================================================
// HvFS v2 直接接口 (internal wrappers)
// ============================================================================

#[no_mangle]
pub fn hvfs_init_internal() {
    let hvfs = get_hvfs();
    if !hvfs.is_initialized() {
        hvfs.init();
    }
}

#[no_mangle]
pub fn hvfs_format_internal() -> i32 {
    let hvfs = get_hvfs();
    let (drive_id, part_start) = hvfs.drives_discovered.lock().first().copied().unwrap_or((
        hvfs.disk_drive.load(core::sync::atomic::Ordering::Acquire),
        hvfs.partition_start
            .load(core::sync::atomic::Ordering::Acquire),
    ));
    hvfs.format_drive(drive_id, part_start);
    0
}

#[no_mangle]
pub fn hvfs_check_disk_internal() -> i32 {
    let hvfs = get_hvfs();
    hvfs.is_disk_mode() as i32
}

#[no_mangle]
pub fn hvfs_set_disk_present_internal(present: bool) {
    let hvfs = get_hvfs();
    if present {
        hvfs.spa
            .disk_present
            .store(true, core::sync::atomic::Ordering::Release);
    }
}

#[no_mangle]
pub fn hvfs_open_internal(path: *const u8, flags: u32, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);
    let hvfs = get_hvfs();
    match hvfs.open(path, flags, pwm) {
        Ok(fd) => fd,
        Err(e) => e.as_i32(),
    }
}

#[no_mangle]
pub fn hvfs_close_internal(fd: u32) -> i32 {
    let hvfs = get_hvfs();
    hvfs.close(fd)
}

#[no_mangle]
pub fn hvfs_read_internal(fd: u32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut user_buf = unsafe { UserWritePtr::new(buf, count as usize) };
    let hvfs = get_hvfs();
    hvfs.read(fd, user_buf.as_mut_slice(), count)
}

#[no_mangle]
pub fn hvfs_write_internal(fd: u32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let user_buf = unsafe { UserReadPtr::new(buf, count as usize) };
    let hvfs = get_hvfs();
    hvfs.write(fd, user_buf.as_slice(), count)
}

#[no_mangle]
pub fn hvfs_mkdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);
    let hvfs = get_hvfs();
    hvfs.mkdir(path, pwm)
}

#[no_mangle]
pub fn hvfs_sync_internal() -> i32 {
    let hvfs = get_hvfs();
    hvfs.sync()
}

#[no_mangle]
pub fn hvfs_get_stats_internal(
    total_blocks: *mut u32,
    free_blocks: *mut u32,
    total_nodes: *mut u32,
    free_nodes: *mut u32,
) {
    let hvfs = get_hvfs();
    let (allocs, frees, _reads, _writes) = hvfs.get_stats();
    if !total_blocks.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let mut r = unsafe { UserRefMut::new(total_blocks) };
        *r.as_mut() = allocs as u32;
    }
    if !free_blocks.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let mut r = unsafe { UserRefMut::new(free_blocks) };
        *r.as_mut() = frees as u32;
    }
    if !total_nodes.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let mut r = unsafe { UserRefMut::new(total_nodes) };
        *r.as_mut() = 0;
    }
    if !free_nodes.is_null() {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let mut r = unsafe { UserRefMut::new(free_nodes) };
        *r.as_mut() = 0;
    }
}

#[no_mangle]
pub fn hvfs_set_current_dir_internal(_node_id: u32) {
    let hvfs = get_hvfs();
    hvfs.current_dir
        .store(_node_id as u64, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub fn hvfs_get_current_dir_internal() -> u32 {
    let hvfs = get_hvfs();
    hvfs.current_dir.load(core::sync::atomic::Ordering::Acquire) as u32
}

#[no_mangle]
pub fn hvfs_set_current_pwm_internal(pwm: u64) {
    let hvfs = get_hvfs();
    hvfs.current_pwm
        .store(pwm, core::sync::atomic::Ordering::Release);
}

#[no_mangle]
pub fn hvfs_get_current_pwm_internal() -> u64 {
    let hvfs = get_hvfs();
    hvfs.current_pwm.load(core::sync::atomic::Ordering::Acquire)
}

// ============================================================================
// Barrier 接口
// ============================================================================

#[no_mangle]
pub fn vfs_barrier_capture() {
    VFS_MANAGER.capture_snapshot();
}

#[no_mangle]
pub fn vfs_barrier_restore() -> i32 {
    VFS_MANAGER.restore_from_snapshot();
    1
}

// ============================================================================
// 公共 VFS API
// ============================================================================

#[no_mangle]
pub fn vfs_init() {
    vfs_init_internal();
}

#[no_mangle]
pub fn vfs_mount(path: *const u8, fs_name: *const u8) -> i32 {
    vfs_mount_internal(path, fs_name)
}

#[no_mangle]
pub fn vfs_umount_internal(path: *const u8, _flags: i32) -> i32 {
    if path.is_null() {
        return -22; // -EINVAL
    }
    let path = ptr_to_str(path);
    match VFS_MANAGER.unmount(path) {
        Ok(()) => 0,
        Err(_) => -2, // -ENOENT
    }
}

#[no_mangle]
pub fn vfs_umount(path: *const u8, flags: i32) -> i32 {
    vfs_umount_internal(path, flags)
}

#[no_mangle]
pub fn vfs_open(path: *const u8, flags: u32, pwm: u64) -> i32 {
    vfs_open_internal(path, flags, pwm)
}

#[no_mangle]
pub fn vfs_close(fd: u32) -> i32 {
    vfs_close_internal(fd)
}

#[no_mangle]
pub fn vfs_read(fd: u32, buf: *mut u8, count: u32) -> i32 {
    vfs_read_internal(fd, buf, count)
}

#[no_mangle]
pub fn vfs_write(fd: u32, buf: *const u8, count: u32) -> i32 {
    vfs_write_internal(fd, buf, count)
}

#[no_mangle]
pub fn vfs_stat(path: *const u8, st: *mut VfsStat, pwm: u64) -> i32 {
    vfs_stat_internal(path, st, pwm)
}

/// Safe 包装: services 层用, 返回 VfsStat 而非 raw pointer.
///
/// 内部复用 vfs_stat_internal, 在 stack 上接收结果, 然后转为 Option 返回.
/// 服务层拿到 Option<VfsStat> 后可安全地用 write_struct_to_user 写回 user.
pub fn vfs_stat_safe(path: *const u8, pwm: u64) -> Option<VfsStat> {
    if path.is_null() {
        return None;
    }
    let mut st = VfsStat::default();
    let r = vfs_stat_internal(path, &mut st as *mut VfsStat, pwm);
    if r < 0 {
        None
    } else {
        Some(st)
    }
}

#[no_mangle]
pub fn vfs_mkdir(path: *const u8, pwm: u64) -> i32 {
    vfs_mkdir_internal(path, pwm)
}

#[no_mangle]
pub fn vfs_chmod(path: *const u8, mode: u16, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.chmod(rel_path, mode, pwm)
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.chmod(rel_path, mode, pwm)
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_chown(path: *const u8, owner_pwm: u64, pwm: u64) -> i32 {
    vfs_chown_ext(path, owner_pwm, 0, pwm)
}

#[no_mangle]
pub fn vfs_chown_ext(
    path: *const u8,
    owner_pwm: u64,
    group_pwm: u64,
    pwm: u64,
) -> i32 {
    let path = ptr_to_str(path);
    let pwm = resolve_pwm(pwm);

    let (mount_idx, fs_type) = match VFS_MANAGER.resolve_mount(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    match fs_type {
        FsType::RamFs => {
            let mut ramfs = RAMFS_DATA.lock();
            ramfs.chown_ext(rel_path, owner_pwm, group_pwm, pwm)
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.chown_ext(rel_path, owner_pwm, group_pwm, pwm)
        }

        FsType::Unknown => -1,
    }
}

// ============================================================================
// fchmod — 按 fd 修改文件权限
// ============================================================================

#[no_mangle]
pub fn vfs_fchmod(fd: u32, mode: u16) -> i32 {
    let fd_usize = fd as usize;
    if fd_usize >= 256 {
        return -9;
    }
    let (used, node_id) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        (fd_table[fd_usize].used, fd_table[fd_usize].node_id)
    };
    if !used {
        return -9;
    }
    let mut ramfs = RAMFS_DATA.lock();
    if (node_id as usize) < ramfs.nodes.len() {
        let node = &mut ramfs.nodes[node_id as usize];
        if node.used {
            node.perm = mode;
            return 0;
        }
    }
    -1
}

// ============================================================================
// fchown — 按 fd 修改文件所有者
// ============================================================================

#[no_mangle]
pub fn vfs_fchown(fd: u32, owner_pwm: u64, group_pwm: u64, pwm: u64) -> i32 {
    let fd_usize = fd as usize;
    if fd_usize >= 256 {
        return -9;
    }
    let (used, node_id) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        (fd_table[fd_usize].used, fd_table[fd_usize].node_id)
    };
    if !used {
        return -9;
    }
    let mut ramfs = RAMFS_DATA.lock();
    if (node_id as usize) >= ramfs.nodes.len() {
        return -1;
    }
    let node = &mut ramfs.nodes[node_id as usize];
    if !node.used {
        return -1;
    }
    // 权限检查: 仅 level==0 可修改任意 owner, 否则仅同 owner 可改.
    let level = crate::kernel::framework::credo::api::pwm_get_privilege_level(pwm);
    if level != 0 && node.owner_pwm != pwm {
        return -1;
    }
    node.owner_pwm = owner_pwm;
    if group_pwm != 0 {
        node.group_pwm = group_pwm;
    }
    0
}

#[no_mangle]
pub fn vfs_unlink(path: *const u8, pwm: u64) -> i32 {
    vfs_unlink_internal(path, pwm)
}

/// link(oldpath, newpath) — 创建硬链接.
/// 真实实现: 从 oldpath 解析出 target node, 在 newpath 父目录下建同名 dir entry,
/// 共享同一 inode, link_count + 1.
#[no_mangle]
pub fn vfs_link(oldpath: *const u8, newpath: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(oldpath);
    let new_path = ptr_to_str(newpath);
    if old_path.is_empty() || new_path.is_empty() {
        return -22;
    }
    let pwm_eff = resolve_pwm(pwm);
    // 仅支持 ramfs
    let mut ramfs = RAMFS_DATA.lock();
    let target_node = match ramfs.resolve_path(old_path) {
        Some(n) => n,
        None => return -2,
    };
    if (target_node as usize) >= ramfs.nodes.len() {
        return -22;
    }
    if !ramfs.nodes[target_node as usize].used {
        return -2;
    }
    if ramfs.nodes[target_node as usize].file_type == VfsFileType::Dir as u8 {
        return -1; // EPERM: 目录不允许硬链接
    }
    // 拆 newpath
    let (parent_path, name) = match new_path.rfind('/') {
        Some(0) => ("/", &new_path[1..]),
        Some(pos) => (&new_path[..pos], &new_path[pos + 1..]),
        None => ("/", new_path),
    };
    if name.is_empty() || name.contains('/') {
        return -22;
    }
    let parent_num = match RAMFS_DATA.lock().resolve_path(parent_path) {
        Some(n) => n,
        None => return -2,
    };
    ramfs.link(parent_num, target_node, name, pwm_eff)
}

/// symlink(target, linkpath) — 创建符号链接.
/// 真实实现: 在 linkpath 父目录下建 Symlink 类型新节点, target 存入 symlink_targets.
#[no_mangle]
pub fn vfs_symlink(target: *const u8, linkpath: *const u8, pwm: u64) -> i32 {
    let tgt = ptr_to_str(target);
    let link_path = ptr_to_str(linkpath);
    if tgt.is_empty() || link_path.is_empty() || tgt.len() >= 128 {
        return -22;
    }
    let pwm_eff = resolve_pwm(pwm);
    let (parent_path, name) = match link_path.rfind('/') {
        Some(0) => ("/", &link_path[1..]),
        Some(pos) => (&link_path[..pos], &link_path[pos + 1..]),
        None => ("/", link_path),
    };
    if name.is_empty() || name.contains('/') {
        return -22;
    }
    let mut ramfs = RAMFS_DATA.lock();
    ramfs.symlink(tgt, parent_path, name, pwm_eff)
}

/// readlink(path, buf, bufsiz) — 读取符号链接目标.
/// 真实实现: 解析 path, 若是 Symlink 则读 symlink_targets[node] 写入用户 buf.
/// 写入不带 NUL 终止符, 返写入字节数.
#[no_mangle]
pub fn vfs_readlink(path: *const u8, buf: *mut u8, bufsiz: u64, pwm: u64) -> i32 {
    let _ = pwm;
    let p = ptr_to_str(path);
    if p.is_empty() {
        return -22;
    }
    if buf.is_null() || bufsiz == 0 {
        return -22;
    }
    let node_id = match RAMFS_DATA.lock().resolve_path(p) {
        Some(n) => n,
        None => return -2,
    };
    let ramfs = RAMFS_DATA.lock();
    // SAFETY: buf 经调用方校验, bufsiz 字节可写.
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, bufsiz as usize) };
    ramfs.readlink(node_id, slice)
}

#[no_mangle]
pub fn vfs_rename(old: *const u8, new: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(old);
    let new_path = ptr_to_str(new);
    let pwm = resolve_pwm(pwm);

    let (old_mount_idx, old_fs_type) = match VFS_MANAGER.resolve_mount(old_path) {
        Some(r) => r,
        None => return -1,
    };
    let (new_mount_idx, _new_fs_type) = match VFS_MANAGER.resolve_mount(new_path) {
        Some(r) => r,
        None => return -1,
    };

    // rename 跨卷不支持 (简化)
    if old_mount_idx != new_mount_idx {
        return -22; // E_INVAL
    }

    let old_rel = VFS_MANAGER.get_relative_path(old_path, old_mount_idx);
    let new_rel = VFS_MANAGER.get_relative_path(new_path, new_mount_idx);

    match old_fs_type {
        FsType::RamFs => {
            // RamFS rename: unlink + link (简单实现)
            let mut ramfs = RAMFS_DATA.lock();
            // 遍历查找 node — RamFS 目录使用固定 parent_node
            // 对于 RamFS 根目录, 直接使用 unlink + link
            ramfs.unlink(old_rel, pwm);
            ramfs.link(0, 0, new_rel, pwm) // parent=0, target=0 为占位
        }
        FsType::HvFs => {
            let hvfs = get_hvfs();
            hvfs.rename(old_rel, new_rel, pwm)
        }

        FsType::Unknown => -1,
    }
}

#[no_mangle]
pub fn vfs_rmdir(path: *const u8, pwm: u64) -> i32 {
    vfs_rmdir_internal(path, pwm)
}

#[no_mangle]
pub fn vfs_readdir(fd: u32, entry: *mut VfsDirEntry) -> i32 {
    vfs_readdir_internal(fd, entry)
}

#[no_mangle]
pub fn vfs_sync() -> i32 {
    hvfs_sync_internal()
}

#[no_mangle]
pub fn vfs_get_cwd(buf: *mut u8, size: u32) -> i32 {
    vfs_get_cwd_internal(buf, size)
}

#[no_mangle]
pub fn vfs_set_cwd(path: *const u8) {
    vfs_set_cwd_internal(path);
}

#[no_mangle]
pub fn vfs_seek(fd: u32, offset: i32, whence: u32) -> i32 {
    let whence = VfsSeekWhence::from_u32(whence);
    let fd_info = VFS_MANAGER.get_fd_info(fd as usize);
    let current_offset = fd_info.map(|(_, off, _)| off).unwrap_or(0);

    let (_mount_idx, fs_type) =
        match VFS_MANAGER.resolve_mount(VFS_MANAGER.fd_table.lock()[fd as usize].get_path()) {
            Some(r) => r,
            None => {
                let hvfs = get_hvfs();
                return hvfs.seek(fd, offset as i64, whence as u32) as i32;
            }
        };

    match fs_type {
        FsType::RamFs => {
            let node_id = fd_info.map(|(ino, _, _)| ino).unwrap_or(0);
            let ramfs = RAMFS_DATA.lock();
            match ramfs.seek(node_id, current_offset, offset as i64, whence) {
                Some(new_offset) => {
                    VFS_MANAGER.set_fd_offset(fd as usize, new_offset);
                    new_offset as i32
                }
                None => KernelError::InvalidArgument.as_i32(),
            }
        }
        _ => {
            let hvfs = get_hvfs();
            hvfs.seek(fd, offset as i64, whence as u32) as i32
        }
    }
}

#[no_mangle]
pub fn vfs_fd_table() -> *const u8 {
    VFS_MANAGER.fd_table.lock().as_ptr() as *const u8
}

#[no_mangle]
pub fn vfs_format_internal(path: *const u8, fs_type: *const u8) -> i32 {
    let fs_type_str = ptr_to_str(fs_type);
    let _path = ptr_to_str(path);

    if fs_type_str.is_empty() {
        return -1;
    }

    // Parse filesystem type
    if fs_type_str == "hvfs" || fs_type_str == "HvFS" {
        let hvfs = get_hvfs();
        let (drive_id, part_start) = hvfs.drives_discovered.lock().first().copied().unwrap_or((
            hvfs.disk_drive.load(core::sync::atomic::Ordering::Acquire),
            hvfs.partition_start
                .load(core::sync::atomic::Ordering::Acquire),
        ));
        hvfs.format_drive(drive_id, part_start);
        if hvfs.is_disk_mode() {
            return 0;
        } else {
            return -1;
        }
    } else if fs_type_str == "ramfs" || fs_type_str == "RamFS" {
        // RamFS doesn't need formatting, it's always in-memory
        return 0;
    }

    -1
}

// ============================================================================
// fstat — 从 fd 获取文件属性
// ============================================================================

#[no_mangle]
pub fn vfs_fstat(fd: u32, st: *mut VfsStat, pwm: u64) -> i32 {
    let fd_usize = fd as usize;
    if fd_usize >= 256 {
        return -9;
    }
    let used = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        fd_table[fd_usize].used
    };
    if !used {
        return -9;
    }
    let (node_id, _mount_idx) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        (fd_table[fd_usize].node_id, 0)
    };
    let _pwm = resolve_pwm(pwm);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_ref = unsafe { UserRefMut::new(st) };

    let result = {
        let ramfs = RAMFS_DATA.lock();
        match ramfs.stat(node_id) {
            Some(stat) => {
                *st_ref.as_mut() = stat;
                0
            }
            None => {
                let hvfs = get_hvfs();
                let fd_table = VFS_MANAGER.fd_table.lock();
                let path = fd_table[fd_usize].path;
                let path_str = core::str::from_utf8(
                    &path[..path.iter().position(|&b| b == 0).unwrap_or(256).min(256)],
                )
                .unwrap_or("");
                match hvfs.stat(path_str, pwm) {
                    Some(obj) => {
                        let r = st_ref.as_mut();
                        r.node_id = obj.obj_id as u32;
                        r.mode = obj.pwm_perm;
                        r.size = obj.size as u32;
                        r.owner_pwm = obj.owner_pwm;
                        r.group_pwm = obj.group_pwm;
                        r.perm = obj.pwm_perm;
                        r.sensitivity = obj.sensitivity;
                        r.file_type = if obj.is_dir() {
                            VfsFileType::Dir.as_u8()
                        } else {
                            VfsFileType::File.as_u8()
                        };
                        0
                    }
                    None => -1,
                }
            }
        }
    };

    if result == 0 {
        let tbl = crate::kernel::framework::credo::identity::get_table();
        let r = st_ref.as_mut();
        r.uid = tbl.uid_of(r.owner_pwm);
        r.gid = tbl.gid_of(r.group_pwm);
        if r.gid == 0xFFFF_FFFF {
            r.gid = r.uid;
        }
    }

    result
}

/// Safe 包装: services 层用, 返回 VfsStat 而非 raw pointer.
pub fn vfs_fstat_safe(fd: u32, pwm: u64) -> Option<VfsStat> {
    let mut st = VfsStat::default();
    let r = vfs_fstat(fd, &mut st as *mut VfsStat, pwm);
    if r < 0 {
        None
    } else {
        Some(st)
    }
}

// ============================================================================
// dup / dup2 — 文件描述符复制
// ============================================================================

#[no_mangle]
pub fn vfs_dup(oldfd: u32) -> i32 {
    let old_usize = oldfd as usize;
    if old_usize >= 256 {
        return -9;
    }
    let mut fd_table = VFS_MANAGER.fd_table.lock();
    if !fd_table[old_usize].used {
        return -9;
    }
    for i in 0..256usize {
        if !fd_table[i].used {
            fd_table[i] = fd_table[old_usize].clone();
            return i as i32;
        }
    }
    -24 // EMFILE
}

#[no_mangle]
pub fn vfs_dup2(oldfd: u32, newfd: u32) -> i32 {
    let old_usize = oldfd as usize;
    let new_usize = newfd as usize;
    if old_usize >= 256 || new_usize >= 256 {
        return -9;
    }
    let mut fd_table = VFS_MANAGER.fd_table.lock();
    if !fd_table[old_usize].used {
        return -9;
    }
    if new_usize == old_usize {
        return newfd as i32;
    }
    fd_table[new_usize] = fd_table[old_usize].clone();
    newfd as i32
}
