#![deny(unsafe_code)]
//! 全局 OpenFile 表 — 内核管理的打开文件描述表
//!
//! 存储所有打开的文件描述 (OpenFile), 通过 handle_id 引用.
//! dup() 通过引用计数共享 OpenFile, 实现 POSIX 共享 offset 语义.

use core::sync::atomic::{AtomicU32, Ordering};
use crate::kernel::framework::sync::IrqSpinLock;
use super::vfs_types::OpenFile;

/// 全局 OpenFile 表上限
const MAX_OPEN_FILES: usize = 256;

/// 全局 OpenFile 表
///
/// 存储所有打开的文件描述, 通过 handle_id 引用.
/// handle_id 是全局唯一的, 跨进程共享.
pub struct OpenFileTable {
    /// OpenFile 条目
    entries: IrqSpinLock<[Option<OpenFile>; MAX_OPEN_FILES]>,
    /// 下一个可用的 handle_id
    next_id: AtomicU32,
}

impl OpenFileTable {
    /// 创建未初始化的 OpenFileTable
    pub const fn new() -> Self {
        Self {
            entries: IrqSpinLock::new([None; MAX_OPEN_FILES]),
            next_id: AtomicU32::new(1),
        }
    }

    /// 分配一个新的 OpenFile, 返回 handle_id
    pub fn alloc(&self, file: OpenFile) -> Option<u32> {
        let handle_id = self.next_id.fetch_add(1, Ordering::AcqRel);
        if handle_id as usize >= MAX_OPEN_FILES {
            return None;
        }

        let mut entries = self.entries.lock();
        entries[handle_id as usize] = Some(file);
        Some(handle_id)
    }

    /// 获取 OpenFile 的不可变引用
    pub fn get(&self, handle_id: u32) -> Option<core::cell::Ref<'_, OpenFile>> {
        let entries = self.entries.lock();
        if (handle_id as usize) < MAX_OPEN_FILES {
            if entries[handle_id as usize].is_some() {
                // 由于 IrqSpinLock 不支持 RefCell 风格的借用,
                // 我们直接返回一个包装器
                Some(OpenFileRef { entries, handle_id })
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 增加引用计数 (dup 时调用)
    pub fn inc_ref(&self, handle_id: u32) {
        let entries = self.entries.lock();
        if let Some(file) = entries[handle_id as usize].as_ref() {
            file.inc_ref();
        }
    }

    /// 减少引用计数 (close 时调用)
    /// 如果引用计数降为 0, 移除 OpenFile
    pub fn dec_ref(&self, handle_id: u32) {
        let mut entries = self.entries.lock();
        if let Some(file) = entries[handle_id as usize].as_ref() {
            let remaining = file.dec_ref();
            if remaining == 0 {
                entries[handle_id as usize] = None;
            }
        }
    }

    /// 关闭 handle (减少引用计数, 如果为 0 则释放)
    pub fn close(&self, handle_id: u32) {
        self.dec_ref(handle_id);
    }
}

/// OpenFile 不可变引用包装器
///
/// 由于 IrqSpinLock 不支持 RefCell 风格借用,
/// 我们使用这个包装器提供安全的引用访问.
pub struct OpenFileRef<'a> {
    entries: core::cell::Ref<'a, [Option<OpenFile>; MAX_OPEN_FILES]>,
    handle_id: u32,
}

impl<'a> core::ops::Deref for OpenFileRef<'a> {
    type Target = OpenFile;

    fn deref(&self) -> &Self::Target {
        self.entries[self.handle_id as usize].as_ref().unwrap()
    }
}

/// 全局 OpenFile 表实例
pub static OPEN_FILE_TABLE: OpenFileTable = OpenFileTable::new();
