#![deny(unsafe_code)]
//! Per-process 文件描述符表 (FD 分配策略) — services 层
//!
//! ## 框架责任分离
//!
//! - **framework**: 进程结构、上下文切换、IRQ 安全锁原语
//! - **services** (本模块): FD 分配策略 — 上限、选择算法 (first-fit)
//!
//! ## 与 Linux 差异
//!
//! - Linux 默认上限 1024, 本项目 v1 上限 64 (嵌入式轻量基线)
//! - 分配策略: first-fit (找第一个空闲 slot). 不实现 O(log N) 位图
//!   是因为 64 slot 线性扫描成本 O(64) < 位图查找的间接开销
//!
//! ## 关联
//!
//! - TCB 减面: [docs/plan/maintenance-2026-06-11.md](../../../../../../docs/plan/maintenance-2026-06-11.md) I-01
//! - 移出: framework::proc::process::FdTable (2026-06-11)

use crate::kernel::framework::sync::IrqSpinLock;

/// 每进程 FD 表上限
///
/// v1 嵌入式轻量基线: 64 slot. 后续可按进程配置或扩容.
pub const MAX_FDS_PER_PROCESS: usize = 64;

/// Per-process FD 表
///
/// `entries[local_fd] = global_fd` (Linux 风格的本地 FD 抽象)
/// -1 表示 slot 空闲.
#[derive(Debug)]
pub struct FdTable {
    entries: IrqSpinLock<[i32; MAX_FDS_PER_PROCESS]>,
}

impl FdTable {
    /// 创建未初始化的 FdTable (const fn, 用于 static).
    pub const fn new() -> Self {
        Self {
            entries: IrqSpinLock::new([-1; MAX_FDS_PER_PROCESS]),
        }
    }

    /// 初始化 FD 表 (清空所有 slot).
    pub fn init(&self) {
        let mut entries = self.entries.lock();
        for e in entries.iter_mut() {
            *e = -1;
        }
    }

    /// 分配 per-process FD slot, 返回本地 fd 编号.
    ///
    /// 策略: first-fit (线性扫描, O(MAX_FDS_PER_PROCESS) = O(64)).
    /// 分配失败返回 None (进程已达 FD 上限).
    pub fn alloc_fd(&self, global_fd: i32) -> Option<usize> {
        let mut entries = self.entries.lock();
        for i in 0..MAX_FDS_PER_PROCESS {
            if entries[i] == -1 {
                entries[i] = global_fd;
                return Some(i);
            }
        }
        None
    }

    /// 通过本地 fd 获取全局 FD 编号.
    ///
    /// 越界或空闲 slot 返回 None.
    pub fn get_global_fd(&self, local_fd: usize) -> Option<i32> {
        let entries = self.entries.lock();
        if local_fd < MAX_FDS_PER_PROCESS {
            let gfd = entries[local_fd];
            if gfd != -1 {
                Some(gfd)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// 关闭本地 fd. 成功 (关闭了有效 slot) 返回 true.
    pub fn close_fd(&self, local_fd: usize) -> bool {
        if local_fd >= MAX_FDS_PER_PROCESS {
            return false;
        }
        let mut entries = self.entries.lock();
        if entries[local_fd] != -1 {
            entries[local_fd] = -1;
            true
        } else {
            false
        }
    }
}

impl Default for FdTable {
    fn default() -> Self {
        Self::new()
    }
}
