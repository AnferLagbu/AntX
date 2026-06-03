//! CpuLocal — Per-CPU 变量安全抽象 (TCB)
//!
//! 提供类型安全的 per-CPU 数据访问，内部通过
//! `arch!(cpu_id())` 索引静态槽位数组。
//!
//! ## 与 Asterinas OSTD `CpuLocal` 的关系
//!
//! 等价于 OSTD 的 `cpu_local!()` 宏 + `PerCpu<T>` 类型。
//!
//! ## SAFETY 不变量
//!
//! - 运行时 CPU 数 ≤ MAX_CPUS。
//! - `cpu_id()` 返回值为 [0, MAX_CPUS) 范围内的有效索引。
//! - per-CPU 数据仅在所属 CPU 上访问。

use core::cell::UnsafeCell;

use crate::kernel::config::MAX_CPUS;

/// Per-CPU 数据容器。
///
/// 初始化通过 `init_this_cpu()` 方法，不使用 `const fn new()`。
/// 槽位用 `Option<T>` 管理生命周期：初始化 → 使用 → 释放。
pub struct CpuLocal<T: 'static> {
    slots: [UnsafeCell<Option<T>>; MAX_CPUS],
}

impl<T> CpuLocal<T> {
    /// 创建空的 Per-CPU 数组（所有槽位初始为 None）。
    pub fn new() -> Self {
        let slots: [UnsafeCell<Option<T>>; MAX_CPUS] =
            unsafe { core::mem::zeroed() };
        Self { slots }
    }

    /// 初始化当前 CPU 的数据。
    ///
    /// # Panics
    /// 如果当前 CPU 的槽位已被初始化。
    pub fn init_this_cpu(&self, val: T) {
        let cpu = crate::arch!(cpu_id()) as usize;
        assert!(cpu < MAX_CPUS, "CPU id {} exceeds MAX_CPUS", cpu);
        // SAFETY: 在启动阶段单线程调用，无竞争。每个槽位只初始化一次。
        unsafe {
            let slot = &mut *self.slots[cpu].get();
            assert!(slot.is_none(), "CpuLocal slot {} already initialized", cpu);
            *slot = Some(val);
        }
    }

    /// 获取当前 CPU 数据的只读引用。
    ///
    /// # Panics
    /// 如果当前 CPU 的槽位未初始化。
    pub fn get(&self) -> &T {
        let cpu = crate::arch!(cpu_id()) as usize;
        assert!(cpu < MAX_CPUS);
        unsafe {
            let slot = &*self.slots[cpu].get();
            slot.as_ref().expect("CpuLocal: slot not initialized")
        }
    }

    /// 获取当前 CPU 数据的可变引用。
    ///
    /// # Panics
    /// 如果当前 CPU 的槽位未初始化。
    pub fn get_mut(&self) -> &mut T {
        let cpu = crate::arch!(cpu_id()) as usize;
        assert!(cpu < MAX_CPUS);
        unsafe {
            let slot = &mut *self.slots[cpu].get();
            slot.as_mut().expect("CpuLocal: slot not initialized")
        }
    }

    /// 释放当前 CPU 的槽位，返回数据。
    pub fn take(&self) -> Option<T> {
        let cpu = crate::arch!(cpu_id()) as usize;
        assert!(cpu < MAX_CPUS);
        unsafe {
            let slot = &mut *self.slots[cpu].get();
            slot.take()
        }
    }
}

// SAFETY: 每个 CPU 只访问自己的槽位，无跨 CPU 竞争。
unsafe impl<T: Send> Sync for CpuLocal<T> {}
