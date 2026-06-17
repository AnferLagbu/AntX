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
use crate::kernel::framework::fs::ramfs::ramfs::{RAMFS_DATA, RamFsData};
use crate::kernel::framework::fs::devfs::devfs::DEVFS_DATA;
use crate::kernel::services::fs::devfs::DevfsData;
use crate::kernel::framework::mm::{pcache, PAGE_SIZE};
use crate::kernel::framework::userptr::{UserReadPtr, UserWritePtr, UserRefMut};
use crate::kernel::framework::lib::CStrExt;
use crate::kernel::framework::fd_notify;

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
        FsType::DevFs => {
            // DevFS 初始化由 init()/init_with_chitin_bridge() 完成
        }

        FsType::Unknown => return -1,
    }

    // E6-4: 带 trait object 挂载
    // SAFETY: RAMFS_DATA 和 HVFS_DATA 都是全局静态变量, 其内部数据的实际
    // 生命周期为 'static. Mutex::lock() 返回的 MutexGuard 借用了 &'static Mutex,
    // 因此通过 &*guard 获得的 &RamFsData 实际生命周期为 'static.
    // 这里我们利用这一点将引用提升为 &'static 以存入 VfsMount.
    let fs: &'static dyn FileSystem = match fs_type {
        FsType::RamFs => {
            let guard = RAMFS_DATA.lock();
            // SAFETY: guard 借用 &'static Mutex<RamFsData>, &*guard 生命周期为 'static
            unsafe { &*(&*guard as *const RamFsData) }
        }
        FsType::HvFs => get_hvfs(),
        FsType::DevFs => {
            // SAFETY: DEVFS_DATA 是全局静态变量, &DEVFS_DATA 生命周期为 'static
            unsafe { &*(&DEVFS_DATA as *const DevfsData) }
        }
        _ => return VFS_MANAGER.mount(path, fs_name).as_i32(),
    };
    VFS_MANAGER.mount_with_fs(path, fs_name, fs).as_i32()
}

#[no_mangle]
pub fn vfs_unmount_internal(path: *const u8) -> i32 {
    let path = ptr_to_str(path);
    VFS_MANAGER.unmount(path).as_i32()
}

#[no_mangle]
pub fn vfs_open_internal(path: *const u8, flags: u32, pwm: u64) -> i32 {
    let path = ptr_to_str(path);

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发 (优先于 fs_type match)
    if let Some(fs) = fs_opt {
        let fd_idx = match VFS_MANAGER.alloc_fd() {
            Some(i) => i,
            None => return -1,
        };

        match fs.fs_open(rel_path, flags, pwm) {
            Ok(result) => {
                VFS_MANAGER.set_fd(fd_idx, result.handle, result.offset, flags, pwm, result.file_type, path);
                fd_idx as i32
            }
            Err(KernelError::NotFound) if (flags & VfsOpenFlags::CREAT.bits()) != 0 => {
                // CREAT: 文件不存在, 尝试创建
                let (parent_path, name) = split_parent_name(rel_path);
                match fs.fs_create(parent_path, name, pwm) {
                    Ok(create_result) => {
                        VFS_MANAGER.set_fd(fd_idx, create_result.handle, create_result.offset, flags, pwm, create_result.file_type, path);
                        // inotify: 父目录 IN_CREATE + 新文件 IN_OPEN
                        let parent_ino = fs.fs_resolve_path(parent_path).unwrap_or(0);
                        super::inotify::inotify_notify(parent_ino, super::inotify::IN_CREATE, name, false);
                        super::inotify::inotify_notify(create_result.handle, super::inotify::IN_OPEN, "", false);
                        fd_idx as i32
                    }
                    Err(_) => {
                        VFS_MANAGER.free_fd(fd_idx);
                        -1
                    }
                }
            }
            Err(e) => {
                VFS_MANAGER.free_fd(fd_idx);
                e.as_i32()
            }
        }
    } else {
        // E6-5: fallback 已移除, 所有文件系统均通过 trait object 分发
        KernelError::NotSupported.as_i32()
    }
}

#[no_mangle]
pub fn vfs_close_internal(fd_idx: u32) -> i32 {
    let fd_idx_us = fd_idx as usize;
    if fd_idx_us >= VFS_MAX_FDS {
        return -1;
    }
    // TD-03: 原子 claim-and-clear — 同一把锁内同时快照 node_id/flags 并清 used,
    // 避免双核同时 close 同一 fd 导致 pcache/inotify 二次触发.
    let snapshot = {
        let mut fd_table = VFS_MANAGER.fd_table.lock();
        if !fd_table[fd_idx_us].used {
            None // 已关闭或未使用, 直接返回 0
        } else {
            let snap = (
                fd_table[fd_idx_us].node_id,
                fd_table[fd_idx_us].flags,
            );
            // 在锁内清零 used 标志 — 后续 alloc 不会复用, 杜绝双 close 穿透
            fd_table[fd_idx_us].used = false;
            fd_table[fd_idx_us].fd = 0;
            fd_table[fd_idx_us].node_id = 0;
            fd_table[fd_idx_us].offset = 0;
            Some(snap)
        }
    };
    let (node_id, flags) = match snapshot {
        Some(s) => s,
        None => return 0,
    };
    // B2: 释放该 fd 关联 inode 的全部 pcache 缓存页, 避免内存泄漏
    pcache::pcache_invalidate_inode(node_id);
    // inotify: 文件关闭通知
    let close_mask = if (flags & VfsOpenFlags::WRONLY.bits()) != 0
        || (flags & VfsOpenFlags::RDWR.bits()) != 0
    {
        super::inotify::IN_CLOSE_WRITE
    } else {
        super::inotify::IN_CLOSE_NOWRITE
    };
    super::inotify::inotify_notify(node_id, close_mask, "", false);
    // C1: fd 关闭 → 唤醒该 fd 注册的所有 epoll 等待者 (EPOLLHUP|EPOLLERR)
    fd_notify::notify_fd_close(fd_idx as i32);
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

    let (_, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    // E6-4: trait object 分发 (优先于 fs_type match)
    // 但 pcache 快路径仅 RamFS 支持, 需要特殊处理
    if let Some(fs) = fs_opt {
        // B2: RamFS pcache 快路径 (仅 RamFS + 4KB 对齐)
        if fs.name() == "ramfs" {
            let is_aligned_4k = (count as u64) >= PCACHE_FAST_MIN_BYTES as u64
                && (count as u64) <= PCACHE_FAST_MAX_BYTES as u64
                && (count as u64).is_multiple_of(PAGE_SIZE)
                && (offset as u64).is_multiple_of(PAGE_SIZE);

            if is_aligned_4k {
                let npages = (count as u64 / PAGE_SIZE) as usize;
                let first_pi = offset / PAGE_SIZE;

                let mut all_hit = true;
                for i in 0..npages {
                    if pcache::pcache_lookup(node_id, first_pi + i as u64).is_none() {
                        all_hit = false;
                        break;
                    }
                }

                if all_hit {
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
        }

        // 慢速路径: trait object 分发
        match fs.fs_read(node_id, offset, user_buf.as_mut_slice(), pwm) {
            Ok(n) => {
                VFS_MANAGER.set_fd_offset(fd_idx as usize, offset + n as u64);
                n as i32
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除, 所有文件系统均通过 trait object 分发
        KernelError::NotSupported.as_i32()
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
/// - `pwm`: 权限字; 由 `pwm_has_capability` / `check_privilege` 在内部做权限校验,
///   0 表示无会话,framework 层 ramfs.read 应当返回 EACCES 而非降级为管理员。
///
/// 返回: 实际读取字节数, 负数表示错误.
#[no_mangle]
pub fn vfs_pread_inode(mount_idx: Option<usize>, node_id: u32, file_offset: u64, dst: &mut [u8], pwm: u64) -> i32 {
    // SAFETY: 调用方保证 dst 在生命周期内有效; 长度由调用方控制.
    let mut user_buf = unsafe { UserWritePtr::new(dst.as_mut_ptr(), dst.len()) };

    // P3-I-19: 走 FileSystem trait 分发. 旧实现直接访问 RAMFS_DATA,
    // 非 RamFS (HvFS/DevFS 等) 挂载 mmap 时无法工作. 现按 mount_idx
    // 派发, 无挂载则返回 -1 (EIO). mmap prewarm 由 page_fault 传入
    // vma.mount_idx (mmap 时已解析).
    let mount_idx = match mount_idx {
        Some(i) => i,
        None => return -1,
    };
    let fs = {
        let mounts = VFS_MANAGER.mounts.lock();
        match mounts.get(mount_idx) {
            Some(m) if m.used => m.get_fs(),
            _ => return -1,
        }
    };
    let fs = match fs {
        Some(f) => f,
        None => return -1,
    };
    match fs.fs_pread_inode(node_id, file_offset, user_buf.as_mut_slice(), pwm) {
        Ok(n) => n as i32,
        Err(_) => -1,
    }
}

#[no_mangle]
pub fn vfs_unlink_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // 在删除前获取 inode 号, 用于删除后释放 POSIX 锁
    let ino_before = if let Some(fs) = fs_opt {
        fs.fs_resolve_path(rel_path)
    } else {
        None
    };

    let result = if let Some(fs) = fs_opt {
        match fs.fs_unlink(rel_path, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
    };

    // 文件删除成功后, 释放该 inode 上的 POSIX 锁 + inotify 通知
    if result == 0 {
        if let Some(ino) = ino_before {
            crate::kernel::framework::fs::vfs::flock::posix_lock_release_inode(ino);
            let (parent_path, name) = split_parent_name(rel_path);
            let parent_ino = if let Some(fs) = fs_opt {
                fs.fs_resolve_path(parent_path).unwrap_or(0)
            } else {
                0
            };
            super::inotify::inotify_notify(parent_ino, super::inotify::IN_DELETE, name, false);
            super::inotify::inotify_notify(ino, super::inotify::IN_DELETE_SELF, "", false);
        }
    }

    result
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

    let (_, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        match fs.fs_truncate(_node_id, size, pwm) {
            Ok(()) => {
                super::inotify::inotify_notify(_node_id, super::inotify::IN_MODIFY, "", false);
                0
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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

    let (_, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    // E6-4: trait object 分发 (优先于 fs_type match)
    if let Some(fs) = fs_opt {
        match fs.fs_write(node_id, offset, user_buf.as_slice(), pwm) {
            Ok(n) => {
                VFS_MANAGER.set_fd_offset(fd_idx as usize, offset + n as u64);
                fd_notify::notify_fd_close(fd_idx as i32);
                super::inotify::inotify_notify(node_id, super::inotify::IN_MODIFY, "", false);
                n as i32
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        KernelError::NotSupported.as_i32()
    }
}

#[no_mangle]
pub fn vfs_mkdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    let (parent_path, name) = split_parent_name(rel_path);
    if name.is_empty() {
        return -1;
    }

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        match fs.fs_mkdir(rel_path, pwm) {
            Ok(()) => {
                let parent_ino = fs.fs_resolve_path(parent_path).unwrap_or(0);
                super::inotify::inotify_notify(parent_ino, super::inotify::IN_CREATE, name, true);
                0
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
    }
}

#[no_mangle]
pub fn vfs_rmdir_internal(path: *const u8, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        match fs.fs_rmdir(rel_path, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
    }
}

#[no_mangle]
pub fn vfs_stat_internal(path: *const u8, st: *mut VfsStat, pwm: u64) -> i32 {
    let path = ptr_to_str(path);
    let _pwm = pwm;
    if st.is_null() {
        return -1;
    }
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_ref = unsafe { UserRefMut::new(st) };

    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    let result = if let Some(fs) = fs_opt {
        match fs.fs_stat(rel_path, pwm) {
            Ok(stat) => {
                *st_ref.as_mut() = stat;
                0
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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

    let (_node_id, offset, _pwm, full_path) = match get_fd_info(fd) {
        Some(info) => info,
        None => return -1,
    };

    let (_, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        let mut dir_entry = VfsDirEntry::default();
        match fs.fs_readdir(_node_id, offset, &mut dir_entry) {
            Ok(has_more) => {
                if !has_more {
                    return 0;
                }
                // SAFETY: 调用方保证指针/类型有效
                let mut entry_ref = unsafe { UserRefMut::new(entry) };
                *entry_ref.as_mut() = dir_entry;
                VFS_MANAGER.set_fd_offset(fd as usize, offset + core::mem::size_of::<crate::kernel::framework::fs::ramfs::ramfs::RamFsDirEntry>() as u64);
                1
            }
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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

// I-22: 15 个 `hvfs_*_internal` 函数无调用方, 已随 P3-I-18 迁移至 `vfs_sync` (FileSystem
// trait fs_sync 分发) 后彻底废弃. 旧路径仅 C-FFI 兼容, 无 FFI 调用方, 移除以减小 TCB
// 面积 (约 150 行, 含 unsafe).

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
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        match fs.fs_chmod(rel_path, mode, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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
    let (mount_idx, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(path) {
        Some(r) => r,
        None => return -1,
    };
    let rel_path = VFS_MANAGER.get_relative_path(path, mount_idx);

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        match fs.fs_chown(rel_path, owner_pwm, group_pwm, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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
    let (used, _node_id, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_usize < fd_table.len() && fd_table[fd_usize].used {
            (true, fd_table[fd_usize].node_id, alloc::string::String::from(fd_table[fd_usize].get_path()))
        } else {
            (false, 0, alloc::string::String::new())
        }
    };
    if !used {
        return -9;
    }
    // E6-5: 通过 trait object 分发
    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };
    if let Some(fs) = fs_opt {
        match fs.fs_chmod(&full_path, mode, 0) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        -1
    }
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
    let (used, _node_id, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_usize < fd_table.len() && fd_table[fd_usize].used {
            (true, fd_table[fd_usize].node_id, alloc::string::String::from(fd_table[fd_usize].get_path()))
        } else {
            (false, 0, alloc::string::String::new())
        }
    };
    if !used {
        return -9;
    }
    // E6-5: 通过 trait object 分发
    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
        Some(r) => r,
        None => return -1,
    };
    if let Some(fs) = fs_opt {
        match fs.fs_chown(&full_path, owner_pwm, group_pwm, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        -1
    }
}

#[no_mangle]
pub fn vfs_unlink(path: *const u8, pwm: u64) -> i32 {
    vfs_unlink_internal(path, pwm)
}

/// link(oldpath, newpath) — 创建硬链接.
/// E6-5: 通过 trait object 分发
#[no_mangle]
pub fn vfs_link(oldpath: *const u8, newpath: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(oldpath);
    let new_path = ptr_to_str(newpath);
    if old_path.is_empty() || new_path.is_empty() {
        return -22;
    }
    let pwm_eff = pwm;

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(old_path) {
        Some(r) => r,
        None => return -2,
    };
    if let Some(fs) = fs_opt {
        match fs.fs_link(old_path, new_path, pwm_eff) {
            Ok(()) => 0,
            Err(e) => e.as_i32(),
        }
    } else {
        -1
    }
}

/// symlink(target, linkpath) — 创建符号链接.
/// E6-5: 通过 trait object 分发
#[no_mangle]
pub fn vfs_symlink(target: *const u8, linkpath: *const u8, pwm: u64) -> i32 {
    let tgt = ptr_to_str(target);
    let link_path = ptr_to_str(linkpath);
    if tgt.is_empty() || link_path.is_empty() || tgt.len() >= 128 {
        return -22;
    }
    let pwm_eff = pwm;

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(link_path) {
        Some(r) => r,
        None => return -2,
    };
    if let Some(fs) = fs_opt {
        match fs.fs_symlink(tgt, link_path, pwm_eff) {
            Ok(()) => 0,
            Err(e) => e.as_i32(),
        }
    } else {
        -1
    }
}

/// readlink(path, buf, bufsiz) — 读取符号链接目标.
/// E6-5: 通过 trait object 分发
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

    let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(p) {
        Some(r) => r,
        None => return -2,
    };
    if let Some(fs) = fs_opt {
        // SAFETY: buf 经调用方校验, bufsiz 字节可写.
        let slice = unsafe { core::slice::from_raw_parts_mut(buf, bufsiz as usize) };
        match fs.fs_readlink(p, slice) {
            Ok(n) => n as i32,
            Err(e) => e.as_i32(),
        }
    } else {
        -1
    }
}

#[no_mangle]
pub fn vfs_rename(old: *const u8, new: *const u8, pwm: u64) -> i32 {
    let old_path = ptr_to_str(old);
    let new_path = ptr_to_str(new);

    let (old_mount_idx, _old_fs_type, old_fs_opt) = match VFS_MANAGER.resolve_mount_fs(old_path) {
        Some(r) => r,
        None => return -1,
    };
    let (new_mount_idx, _new_fs_type, _) = match VFS_MANAGER.resolve_mount_fs(new_path) {
        Some(r) => r,
        None => return -1,
    };

    // rename 跨卷不支持 (简化)
    if old_mount_idx != new_mount_idx {
        return -22;
    }

    let old_rel = VFS_MANAGER.get_relative_path(old_path, old_mount_idx);
    let new_rel = VFS_MANAGER.get_relative_path(new_path, new_mount_idx);

    // E6-4: trait object 分发
    if let Some(fs) = old_fs_opt {
        match fs.fs_rename(old_rel, new_rel, pwm) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    } else {
        // E6-5: fallback 已移除
        -1
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
    // P3-I-18: 遍历所有挂载点, 通过 FileSystem trait 的 fs_sync 分发.
    // 替换原 hvfs_sync_internal() 单 FS 写死的实现.
    let mounts = VFS_MANAGER.mounts.lock();
    let mut last_err: i32 = 0;
    let mut synced: u32 = 0;
    for i in 0..VFS_MAX_MOUNTS {
        let m = &mounts[i];
        if !m.used {
            continue;
        }
        if let Some(fs) = m.get_fs() {
            // SAFETY: 见本函数旧实现, fs_sync 不引用 raw pointer, 是
            // 内部纯粹计算 (HvFS 走 txg commit). 互斥由 VFS_MANAGER 维护.
            match fs.fs_sync() {
                Ok(()) => {
                    synced += 1;
                }
                Err(e) => {
                    last_err = e.as_i32();
                    // 继续遍历其它挂载点, 不因单个 FS 失败而中断
                }
            }
        }
    }
    if synced == 0 {
        // 没有任何挂载点: 仍然返回 0 保持兼容性 (老代码语义)
        // 业务上 mount 0 个 FS 时 vfs_sync 是 no-op
    }
    last_err
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

    let path = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        alloc::string::String::from(fd_table[fd as usize].get_path())
    };
    let (_, _fs_type, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&path) {
        Some(r) => r,
        None => return KernelError::InvalidArgument.as_i32(),
    };

    // E6-4: trait object 分发
    if let Some(fs) = fs_opt {
        let node_id = fd_info.map(|(ino, _, _)| ino).unwrap_or(0);
        match fs.fs_seek(node_id, offset as i64, whence, current_offset) {
            Ok(new_offset) => {
                VFS_MANAGER.set_fd_offset(fd as usize, new_offset);
                new_offset as i32
            }
            Err(_) => KernelError::InvalidArgument.as_i32(),
        }
    } else {
        // E6-5: fallback 已移除
        KernelError::InvalidArgument.as_i32()
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
        // RamFS 无需格式化, 始终为内存文件系统
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
    let (_node_id, _mount_idx, full_path) = {
        let fd_table = VFS_MANAGER.fd_table.lock();
        if fd_usize < fd_table.len() && fd_table[fd_usize].used {
            (fd_table[fd_usize].node_id, 0, alloc::string::String::from(fd_table[fd_usize].get_path()))
        } else {
            (0, 0, alloc::string::String::new())
        }
    };
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let mut st_ref = unsafe { UserRefMut::new(st) };

    // E6-5: 通过 trait object 分发
    let result = if !full_path.is_empty() {
        let (_, _, fs_opt) = match VFS_MANAGER.resolve_mount_fs(&full_path) {
            Some(r) => r,
            None => return -1,
        };
        if let Some(fs) = fs_opt {
            match fs.fs_stat(&full_path, pwm) {
                Ok(stat) => {
                    *st_ref.as_mut() = stat;
                    0
                }
                Err(_) => -1,
            }
        } else {
            -1
        }
    } else {
        -1
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
