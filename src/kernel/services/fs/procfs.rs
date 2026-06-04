//! @SAFE: 本文件不含 unsafe 代码。
//!
//! 进程文件系统 (ProcFS) — services 层安全代理 (Phase 2.2.3)
//!
//! 在 `kernel/fs/procfs` 基础上提供 100% safe 的公共 API,
//! 暴露 /proc 虚拟文件系统 (current/sys/cpu/sys/memory/sys/config/...).
//!
//! ## 设计原则
//!
//! - **零 unsafe**: 内部 `ProcfsData` 已由 P2.2.3 标记为安全 (Send+Sync)
//! - **类型安全**: 条目类型用 `ProcEntryKind` 枚举, 而非裸 `u8`
//! - **薄包装**: 透传 mount/add_process/remove_process/read/readdir
//! - **可替代**: 原 `kernel/fs/procfs/procfs.rs` 仍存在, 本文件是迁移目标
//!
//! 评估日期: 2026-06-04
//! Phase 2.2.3 任务: 进程文件系统迁移

use crate::kernel::fs::procfs::procfs::{ProcfsData, PROCFS_MAX_NAME};

// ============================================================================
// 条目类型
// ============================================================================

/// ProcFS 条目类型 (强类型枚举)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcEntryKind {
    /// 普通目录
    Dir = 1,
    /// 当前进程 (self)
    Current = 2,
    /// 进程条目
    Process = 3,
    /// 文件
    File = 4,
}

impl ProcEntryKind {
    /// 从 `u8` 解析
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Dir),
            2 => Some(Self::Current),
            3 => Some(Self::Process),
            4 => Some(Self::File),
            _ => None,
        }
    }
}

/// ProcFS 条目 (服务层视图)
#[derive(Debug, Clone)]
pub struct ProcEntry {
    /// 条目名
    pub name: alloc::string::String,
    /// 关联 PID (process 条目用)
    pub pid: u32,
    /// 条目类型
    pub kind: ProcEntryKind,
}

// ============================================================================
// 安全 ProcFS 代理
// ============================================================================

/// ProcFS 安全代理 (services 层)。
pub struct SafeProcFs {
    inner: &'static ProcfsData,
}

impl SafeProcFs {
    /// 创建全局 ProcFS 代理
    pub fn new() -> Self {
        Self {
            inner: &crate::kernel::fs::procfs::procfs::PROCFS_DATA,
        }
    }

    /// 挂载 ProcFS
    ///
    /// # 返回
    /// 成功: Ok(()); 失败: `&'static str` 错误信息
    pub fn mount(&self, mount_point: &str) -> Result<(), &'static str> {
        let rc = self.inner.mount(mount_point);
        if rc == 0 {
            Ok(())
        } else {
            Err("mount failed")
        }
    }

    /// 注册进程
    pub fn add_process(&self, pid: u32, name: &str) -> Result<(), &'static str> {
        let rc = self.inner.add_process(pid, name);
        if rc == 0 {
            Ok(())
        } else {
            Err("procfs table full")
        }
    }

    /// 注销进程
    pub fn remove_process(&self, pid: u32) -> Result<(), &'static str> {
        let rc = self.inner.remove_process(pid);
        if rc == 0 {
            Ok(())
        } else {
            Err("process not found")
        }
    }

    /// 读条目内容
    ///
    /// # 返回
    /// 成功: 实际写入 `buf` 的字节数
    pub fn read(&self, name: &str, buf: &mut [u8]) -> Result<usize, &'static str> {
        let rc = self.inner.read(name, buf);
        if rc < 0 {
            Err("read failed")
        } else {
            Ok(rc as usize)
        }
    }

    /// 枚举条目
    pub fn readdir(&self, index: usize) -> Option<ProcEntry> {
        let (raw_name, pid, raw_kind) = self.inner.readdir(index)?;
        let kind = ProcEntryKind::from_u8(raw_kind)?;
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(raw_name.len());
        let name = alloc::string::String::from_utf8_lossy(&raw_name[..end]).into_owned();
        Some(ProcEntry { name, pid, kind })
    }

    /// 条目总数
    pub fn entry_count(&self) -> u32 {
        self.inner.entry_count()
    }
}

impl Default for SafeProcFs {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 全局实例
// ============================================================================

use spin::Once;
static GLOBAL_PROCFS: Once<SafeProcFs> = Once::new();

/// 初始化全局 ProcFS
pub fn init_global() {
    GLOBAL_PROCFS.call_once(|| SafeProcFs::new());
}

/// 获取全局 ProcFS 引用
///
/// # Safety
/// 调用前需保证 `init_global` 已执行
pub fn global() -> &'static SafeProcFs {
    GLOBAL_PROCFS
        .get()
        .expect("procfs::global() called before init_global()")
}

// ============================================================================
// 便利函数
// ============================================================================

/// 注册进程到全局 ProcFS
pub fn add_process(pid: u32, name: &str) -> Result<(), &'static str> {
    global().add_process(pid, name)
}

/// 注销进程
pub fn remove_process(pid: u32) -> Result<(), &'static str> {
    global().remove_process(pid)
}

/// 读全局 ProcFS 条目
pub fn read(name: &str, buf: &mut [u8]) -> Result<usize, &'static str> {
    global().read(name, buf)
}

/// 条目名最大长度
pub const fn max_name_len() -> usize {
    PROCFS_MAX_NAME
}
