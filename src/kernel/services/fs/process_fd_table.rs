#![deny(unsafe_code)]
//! Per-process FD 表 — 进程级文件描述符管理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::fs::vfs::api。
//!
//! ## 职责
//!
//! - 管理每个进程的文件描述符表
//! - 支持 POSIX dup 语义 (共享 OpenFile)
//! - 支持 fork 时 FD 表复制
//! - 支持 exec 时 FD 表清理 (CLOEXEC)
//!
//! ## 设计
//!
//! 使用固定大小数组存储 FD 条目, 每个条目包含:
//! - handle_id: 指向全局 OpenFile 表的索引
//! - cloexec: FD 级标志 (CLOEXEC)
//! - used: 是否使用中
//!
//! dup() 通过复制 handle_id 实现, 增加 OpenFile 引用计数.

use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::kernel::framework::syscall::Errno;

/// 每进程 FD 表上限
const MAX_FD_PER_PROCESS: usize = 256;

/// FD 条目
#[derive(Debug, Clone, Copy)]
pub struct FdEntry {
    /// 指向全局 OpenFile 表的索引 (0 = 空闲)
    pub handle_id: u32,
    /// FD 级标志 (CLOEXEC, 不随 dup 共享)
    pub cloexec: bool,
    /// 是否使用中
    pub used: bool,
}

impl FdEntry {
    pub const fn new() -> Self {
        Self {
            handle_id: 0,
            cloexec: false,
            used: false,
        }
    }
}

/// Per-process FD 表
pub struct ProcessFdTable {
    /// FD 条目数组
    entries: Mutex<[FdEntry; MAX_FD_PER_PROCESS]>,
    /// 下一个可用 FD 编号 (从 3 开始, 0/1/2 保留给 stdin/stdout/stderr)
    next_fd: core::sync::atomic::AtomicU32,
}

impl ProcessFdTable {
    /// 创建新的 FD 表
    pub fn new() -> Self {
        Self {
            entries: Mutex::new([FdEntry::new(); MAX_FD_PER_PROCESS]),
            next_fd: core::sync::atomic::AtomicU32::new(3),
        }
    }

    /// 分配一个新的 FD
    pub fn alloc_fd(&self, handle_id: u32, cloexec: bool) -> Option<u32> {
        let fd = self.next_fd.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
        if fd as usize >= MAX_FD_PER_PROCESS {
            return None;
        }

        let mut entries = self.entries.lock();
        entries[fd as usize] = FdEntry {
            handle_id,
            cloexec,
            used: true,
        };
        Some(fd)
    }

    /// 获取 FD 条目
    pub fn get_fd(&self, fd: u32) -> Option<FdEntry> {
        let entries = self.entries.lock();
        if (fd as usize) < MAX_FD_PER_PROCESS && entries[fd as usize].used {
            Some(entries[fd as usize])
        } else {
            None
        }
    }

    /// 关闭 FD
    pub fn close_fd(&self, fd: u32) -> Option<u32> {
        let mut entries = self.entries.lock();
        if (fd as usize) < MAX_FD_PER_PROCESS && entries[fd as usize].used {
            let handle_id = entries[fd as usize].handle_id;
            entries[fd as usize] = FdEntry::new();
            Some(handle_id)
        } else {
            None
        }
    }

    /// 复制 FD (dup 语义)
    pub fn dup_fd(&self, old_fd: u32) -> Option<u32> {
        let entries = self.entries.lock();
        if (old_fd as usize) < MAX_FD_PER_PROCESS && entries[old_fd as usize].used {
            let handle_id = entries[old_fd as usize].handle_id;
            let cloexec = entries[old_fd as usize].cloexec;
            drop(entries);

            // 增加 OpenFile 引用计数
            crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE.inc_ref(handle_id);

            // 分配新 FD
            self.alloc_fd(handle_id, cloexec)
        } else {
            None
        }
    }

    /// 复制 FD 到指定编号 (dup2 语义)
    pub fn dup2_fd(&self, old_fd: u32, new_fd: u32) -> Result<u32, Errno> {
        if new_fd as usize >= MAX_FD_PER_PROCESS {
            return Err(Errno::EBADF);
        }

        let entries = self.entries.lock();
        if (old_fd as usize) < MAX_FD_PER_PROCESS && entries[old_fd as usize].used {
            let handle_id = entries[old_fd as usize].handle_id;
            let cloexec = entries[old_fd as usize].cloexec;

            // 如果 new_fd 已打开, 先关闭
            if entries[new_fd as usize].used {
                let old_handle_id = entries[new_fd as usize].handle_id;
                crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE.dec_ref(old_handle_id);
            }

            // 增加 OpenFile 引用计数
            crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE.inc_ref(handle_id);

            // 设置新 FD
            let mut entries = self.entries.lock();
            entries[new_fd as usize] = FdEntry {
                handle_id,
                cloexec,
                used: true,
            };

            Ok(new_fd)
        } else {
            Err(Errno::EBADF)
        }
    }

    /// 关闭所有 CLOEXEC FD
    pub fn close_cloexec_fds(&self) {
        let mut entries = self.entries.lock();
        for fd in entries.iter_mut() {
            if fd.used && fd.cloexec {
                crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE.dec_ref(fd.handle_id);
                *fd = FdEntry::new();
            }
        }
    }

    /// 清空 FD 表 (exec 时调用, 保留 CLOEXEC FD)
    pub fn clear_non_cloexec(&self) {
        let mut entries = self.entries.lock();
        for fd in entries.iter_mut() {
            if fd.used && !fd.cloexec {
                crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE.dec_ref(fd.handle_id);
                *fd = FdEntry::new();
            }
        }
    }

    /// 获取已使用的 FD 数量
    pub fn used_count(&self) -> u32 {
        let entries = self.entries.lock();
        entries.iter().filter(|fd| fd.used).count() as u32
    }
}

/// 全局默认 FD 表 (用于初始化)
pub static DEFAULT_FD_TABLE: ProcessFdTable = ProcessFdTable {
    entries: Mutex::new([FdEntry::new(); MAX_FD_PER_PROCESS]),
    next_fd: core::sync::atomic::AtomicU32::new(3),
};
