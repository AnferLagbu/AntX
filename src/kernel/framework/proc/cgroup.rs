//! cgroup (Control Group) — 资源限制、统计与隔离
//!
//! ## 设计
//!
//! 采用 cgroup v1 风格的扁平控制器模型, 每种资源 (CPU/内存/PID/IO) 独立控制器.
//! 后续可扩展为 v2 统一层级.
//!
//! ### 核心概念
//!
//! - **CgroupRq**: 一个 cgroup 实例, 包含进程列表 + 各控制器状态
//! - **CgroupSubsystem**: 全局管理器, 维护 cgroup 层级树
//! - **CgroupController**: 控制器 trait, 定义资源限制/统计接口
//!
//! ### 控制器
//!
//! | 控制器 | 限制项 | 机制 |
//! |--------|--------|------|
//! | cpu    | cfs_quota_us / cfs_period_us | CFS 带宽限流 |
//! | memory | limit_in_bytes              | OOM 集成 + mmap 拒绝 |
//! | pids   | pids.max                    | fork 拒绝 |
//! | io     | io.max (rbps/wbps/riops/wiops) | bio 限速 |
//!
//! ### 与 Linux 的差异
//!
//! 1. 不实现 cgroupfs 挂载 (暂用 syscall 管控)
//! 2. 层级深度固定 2 层 (root → child), 不支持任意嵌套
//! 3. 无 migration 权限检查 (内核态唯一信任域)
//!
//! ## SAFETY
//!
//! 本模块属于 framework/TCB, 允许 unsafe.
//! 所有 CgroupRq 内部可变性通过 Mutex 保护.

use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::kernel::framework::proc::types::Pid;

// ============================================================================
// 常量
// ============================================================================

/// cgroup 最大层级深度
pub const CGROUP_MAX_DEPTH: usize = 2;
/// 单个 cgroup 最大进程数
pub const CGROUP_MAX_PROCS: u32 = 4096;
/// 默认 CPU 周期 (1s, 单位 us)
pub const CPU_CFS_PERIOD_DEFAULT_US: u64 = 1_000_000;
/// 默认 CPU 配额 (不限制)
pub const CPU_CFS_QUOTA_MAX: u64 = u64::MAX;
/// 默认内存限制 (不限制)
pub const MEMORY_LIMIT_MAX: u64 = u64::MAX;
/// 默认 PID 最大数 (不限制)
pub const PIDS_MAX_DEFAULT: u64 = u64::MAX;

// ============================================================================
// Cgroup ID 分配
// ============================================================================

static NEXT_CGROUP_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_cgroup_id() -> u64 {
    NEXT_CGROUP_ID.fetch_add(1, Ordering::Relaxed)
}

// ============================================================================
// CPU 控制器
// ============================================================================

/// CPU 控制器状态
///
/// 通过 CFS 带宽限流实现 CPU 限额:
/// - `cfs_quota_us`: 一个周期内允许运行的微秒数
/// - `cfs_period_us`: 带宽周期 (微秒)
/// - 配额耗尽后, cgroup 内所有进程被 throttle
#[derive(Debug)]
pub struct CpuController {
    /// 一个周期内允许运行的微秒数 (MAX = 不限制)
    pub cfs_quota_us: AtomicU64,
    /// 带宽周期 (微秒)
    pub cfs_period_us: AtomicU64,
    /// 本周期已消耗的微秒数
    pub runtime_used: AtomicU64,
    /// 累计被 throttle 次数
    pub nr_throttled: AtomicU64,
    /// 累计 throttle 持续时间 (us)
    pub throttled_time: AtomicU64,
}

impl CpuController {
    pub fn new() -> Self {
        Self {
            cfs_quota_us: AtomicU64::new(CPU_CFS_QUOTA_MAX),
            cfs_period_us: AtomicU64::new(CPU_CFS_PERIOD_DEFAULT_US),
            runtime_used: AtomicU64::new(0),
            nr_throttled: AtomicU64::new(0),
            throttled_time: AtomicU64::new(0),
        }
    }

    /// 检查 cgroup 是否还有 CPU 预算
    ///
    /// 返回 true 表示允许运行, false 表示应 throttle
    pub fn check_budget(&self, delta_us: u64) -> bool {
        let quota = self.cfs_quota_us.load(Ordering::Acquire);
        // 不限制
        if quota == CPU_CFS_QUOTA_MAX {
            return true;
        }
        let used = self.runtime_used.fetch_add(delta_us, Ordering::AcqRel);
        used + delta_us <= quota
    }

    /// 周期重置: 重置 runtime_used
    pub fn period_reset(&self) {
        let quota = self.cfs_quota_us.load(Ordering::Acquire);
        if quota != CPU_CFS_QUOTA_MAX {
            let prev = self.runtime_used.swap(0, Ordering::AcqRel);
            if prev > quota {
                self.nr_throttled.fetch_add(1, Ordering::Relaxed);
                self.throttled_time.fetch_add(prev - quota, Ordering::Relaxed);
            }
        }
    }

    /// 设置 CPU 配额
    pub fn set_quota(&self, quota_us: u64) {
        self.cfs_quota_us.store(quota_us, Ordering::Release);
    }

    /// 设置 CPU 周期
    pub fn set_period(&self, period_us: u64) {
        if period_us > 0 {
            self.cfs_period_us.store(period_us, Ordering::Release);
        }
    }
}

// ============================================================================
// 内存控制器
// ============================================================================

/// 内存控制器状态
///
/// 限制 cgroup 内进程的总内存使用量, 超限时:
/// 1. 触发 OOMD 压力升级
/// 2. 拒绝新 mmap / brk 增长
/// 3. Emergency 时 kill 最大 RSS 进程
#[derive(Debug)]
pub struct MemoryController {
    /// 内存使用上限 (字节), MAX = 不限制
    pub limit_in_bytes: AtomicU64,
    /// 当前已使用内存 (字节)
    pub usage_in_bytes: AtomicU64,
    /// 历史最大使用量 (字节)
    pub max_usage_in_bytes: AtomicU64,
    /// OOM kill 累计次数
    pub oom_kill_count: AtomicU64,
    /// 是否禁用 OOM kill (改为阻塞)
    pub oom_kill_disable: AtomicU32,
}

impl MemoryController {
    pub fn new() -> Self {
        Self {
            limit_in_bytes: AtomicU64::new(MEMORY_LIMIT_MAX),
            usage_in_bytes: AtomicU64::new(0),
            max_usage_in_bytes: AtomicU64::new(0),
            oom_kill_count: AtomicU64::new(0),
            oom_kill_disable: AtomicU32::new(0),
        }
    }

    /// 尝试充值内存 (charge), 返回 true 表示在限额内
    pub fn try_charge(&self, bytes: u64) -> bool {
        let limit = self.limit_in_bytes.load(Ordering::Acquire);
        if limit == MEMORY_LIMIT_MAX {
            // 不限制: 直接充值
            let new = self.usage_in_bytes.fetch_add(bytes, Ordering::AcqRel) + bytes;
            self.update_max(new);
            return true;
        }
        // CAS 循环: 确保不超限
        loop {
            let current = self.usage_in_bytes.load(Ordering::Acquire);
            if current + bytes > limit {
                return false;
            }
            match self.usage_in_bytes.compare_exchange_weak(
                current,
                current + bytes,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.update_max(current + bytes);
                    return true;
                }
                Err(_) => continue,
            }
        }
    }

    /// 释放内存 (uncharge)
    pub fn uncharge(&self, bytes: u64) {
        let prev = self.usage_in_bytes.fetch_sub(bytes, Ordering::AcqRel);
        // 防止下溢
        if prev < bytes {
            self.usage_in_bytes.store(0, Ordering::Release);
        }
    }

    /// 设置内存限制
    pub fn set_limit(&self, limit_bytes: u64) {
        self.limit_in_bytes.store(limit_bytes, Ordering::Release);
    }

    /// 检查是否超限
    pub fn is_over_limit(&self) -> bool {
        let limit = self.limit_in_bytes.load(Ordering::Acquire);
        if limit == MEMORY_LIMIT_MAX {
            return false;
        }
        self.usage_in_bytes.load(Ordering::Acquire) > limit
    }

    fn update_max(&self, current: u64) {
        loop {
            let max = self.max_usage_in_bytes.load(Ordering::Acquire);
            if current <= max {
                break;
            }
            match self.max_usage_in_bytes.compare_exchange_weak(
                max,
                current,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(_) => continue,
            }
        }
    }
}

// ============================================================================
// PID 控制器
// ============================================================================

/// PID 控制器状态
///
/// 限制 cgroup 内的进程数, 超限时 fork 失败 (-EAGAIN)
#[derive(Debug)]
pub struct PidsController {
    /// 最大进程数, MAX = 不限制
    pub pids_max: AtomicU64,
    /// 当前进程数
    pub current: AtomicU64,
    /// fork 被拒绝次数
    pub events_fork_fail: AtomicU64,
}

impl PidsController {
    pub fn new() -> Self {
        Self {
            pids_max: AtomicU64::new(PIDS_MAX_DEFAULT),
            current: AtomicU64::new(0),
            events_fork_fail: AtomicU64::new(0),
        }
    }

    /// 尝试分配一个 PID 槽位, 返回 true 表示在限额内
    pub fn try_fork(&self) -> bool {
        let max = self.pids_max.load(Ordering::Acquire);
        if max == PIDS_MAX_DEFAULT {
            self.current.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        loop {
            let cur = self.current.load(Ordering::Acquire);
            if cur >= max {
                self.events_fork_fail.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            match self.current.compare_exchange_weak(
                cur,
                cur + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }
    }

    /// 释放一个 PID 槽位 (进程退出)
    pub fn exit(&self) {
        let prev = self.current.fetch_sub(1, Ordering::AcqRel);
        if prev == 0 {
            self.current.store(0, Ordering::Release);
        }
    }

    /// 设置最大进程数
    pub fn set_max(&self, max: u64) {
        self.pids_max.store(max, Ordering::Release);
    }
}

// ============================================================================
// IO 控制器
// ============================================================================

/// IO 控制器状态
///
/// 基于 bio 统计的 IO 限速:
/// - 读/写带宽 (bytes/s)
/// - 读/写 IOPS
#[derive(Debug)]
pub struct IoController {
    /// 读带宽上限 (bytes/s), MAX = 不限制
    pub read_bps_max: AtomicU64,
    /// 写带宽上限 (bytes/s), MAX = 不限制
    pub write_bps_max: AtomicU64,
    /// 读 IOPS 上限, MAX = 不限制
    pub read_iops_max: AtomicU64,
    /// 写 IOPS 上限, MAX = 不限制
    pub write_iops_max: AtomicU64,
    /// 统计: 累计读字节数
    pub stat_read_bytes: AtomicU64,
    /// 统计: 累计写字节数
    pub stat_write_bytes: AtomicU64,
    /// 统计: 累计读 IO 次数
    pub stat_read_ios: AtomicU64,
    /// 统计: 写 IO 次数
    pub stat_write_ios: AtomicU64,
}

impl IoController {
    pub fn new() -> Self {
        Self {
            read_bps_max: AtomicU64::new(u64::MAX),
            write_bps_max: AtomicU64::new(u64::MAX),
            read_iops_max: AtomicU64::new(u64::MAX),
            write_iops_max: AtomicU64::new(u64::MAX),
            stat_read_bytes: AtomicU64::new(0),
            stat_write_bytes: AtomicU64::new(0),
            stat_read_ios: AtomicU64::new(0),
            stat_write_ios: AtomicU64::new(0),
        }
    }

    /// 记录一次读 IO
    pub fn account_read(&self, bytes: u64) {
        self.stat_read_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.stat_read_ios.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录一次写 IO
    pub fn account_write(&self, bytes: u64) {
        self.stat_write_bytes.fetch_add(bytes, Ordering::Relaxed);
        self.stat_write_ios.fetch_add(1, Ordering::Relaxed);
    }
}

// ============================================================================
// CgroupRq — cgroup 实例
// ============================================================================

/// cgroup 实例 (cgroup request queue)
///
/// 每个 cgroup 对应一个 CgroupRq, 包含:
/// - 进程列表
/// - 各控制器状态
/// - 层级关系
#[derive(Debug)]
pub struct CgroupRq {
    /// 唯一 ID
    pub id: u64,
    /// cgroup 名称
    pub name: IrqSpinLock<String>,
    /// 父 cgroup ID (0 = root)
    pub parent_id: u64,
    /// 子 cgroup ID 列表
    pub children: IrqSpinLock<Vec<u64>>,
    /// 属于本 cgroup 的进程 PID 列表
    pub procs: IrqSpinLock<Vec<Pid>>,
    /// CPU 控制器
    pub cpu: CpuController,
    /// 内存控制器
    pub memory: MemoryController,
    /// PID 控制器
    pub pids: PidsController,
    /// IO 控制器
    pub io: IoController,
}

// 使用 spin::Mutex 替代 std (与项目风格一致)
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

impl CgroupRq {
    /// 创建根 cgroup
    pub fn new_root() -> Self {
        Self {
            id: 0,
            name: IrqSpinLock::new(String::from("/")),
            parent_id: 0,
            children: IrqSpinLock::new(Vec::new()),
            procs: IrqSpinLock::new(Vec::new()),
            cpu: CpuController::new(),
            memory: MemoryController::new(),
            pids: PidsController::new(),
            io: IoController::new(),
        }
    }

    /// 创建子 cgroup
    pub fn new_child(name: &str, parent_id: u64) -> Self {
        Self {
            id: alloc_cgroup_id(),
            name: IrqSpinLock::new(String::from(name)),
            parent_id,
            children: IrqSpinLock::new(Vec::new()),
            procs: IrqSpinLock::new(Vec::new()),
            cpu: CpuController::new(),
            memory: MemoryController::new(),
            pids: PidsController::new(),
            io: IoController::new(),
        }
    }

    /// 将进程加入本 cgroup
    pub fn attach_proc(&self, pid: Pid) -> bool {
        let mut procs = self.procs.lock();
        if procs.len() >= CGROUP_MAX_PROCS as usize {
            return false;
        }
        if procs.contains(&pid) {
            return true; // 已存在
        }
        procs.push(pid);
        // PID 控制器计数
        self.pids.try_fork()
    }

    /// 将进程移出本 cgroup
    pub fn detach_proc(&self, pid: Pid) {
        let mut procs = self.procs.lock();
        if let Some(pos) = procs.iter().position(|&p| p == pid) {
            procs.swap_remove(pos);
            self.pids.exit();
        }
    }
}

// ============================================================================
// CgroupSubsystem — 全局管理器
// ============================================================================

/// cgroup 全局管理器
///
/// 维护 cgroup 层级树, 提供 cgroup CRUD 和进程迁移.
pub struct CgroupSubsystem {
    /// 所有 cgroup 实例 (id → Arc<CgroupRq>)
    groups: IrqSpinLock<BTreeMap<u64, Arc<CgroupRq>>>,
    /// 根 cgroup (id=0)
    root: Arc<CgroupRq>,
}

impl CgroupSubsystem {
    /// 创建 cgroup 子系统 (含根 cgroup)
    pub fn new() -> Self {
        let root = Arc::new(CgroupRq::new_root());
        let mut groups = BTreeMap::new();
        groups.insert(0, Arc::clone(&root));

        Self {
            groups: IrqSpinLock::new(groups),
            root,
        }
    }

    /// 获取根 cgroup
    pub fn root(&self) -> &Arc<CgroupRq> {
        &self.root
    }

    /// 创建子 cgroup
    ///
    /// `parent_id`: 父 cgroup ID (0 = root)
    /// `name`: cgroup 名称
    /// 返回新 cgroup 的 ID
    pub fn create_cgroup(&self, parent_id: u64, name: &str) -> u64 {
        let mut groups = self.groups.lock();

        // 检查父 cgroup 存在
        if !groups.contains_key(&parent_id) {
            return 0;
        }

        // 检查层级深度
        if parent_id != 0 {
            // 只允许 root → child, 不允许更深嵌套
            if let Some(parent) = groups.get(&parent_id) {
                if parent.parent_id != 0 {
                    return 0; // 超过最大深度
                }
            }
        }

        let cg = Arc::new(CgroupRq::new_child(name, parent_id));
        let id = cg.id;

        // 加入父的 children 列表
        if let Some(parent) = groups.get(&parent_id) {
            parent.children.lock().push(id);
        }

        groups.insert(id, cg);
        id
    }

    /// 删除 cgroup (必须无进程、无子 cgroup)
    pub fn remove_cgroup(&self, id: u64) -> Result<(), Errno> {
        if id == 0 {
            return Err(Errno::EBUSY); // 不能删除根
        }

        let mut groups = self.groups.lock();
        let cg = match groups.get(&id) {
            Some(c) => Arc::clone(c),
            None => return Err(Errno::ENOENT),
        };

        // 检查无进程
        if !cg.procs.lock().is_empty() {
            return Err(Errno::EBUSY);
        }

        // 检查无子 cgroup
        if !cg.children.lock().is_empty() {
            return Err(Errno::EBUSY);
        }

        // 从父的 children 中移除
        if let Some(parent) = groups.get(&cg.parent_id) {
            let mut children = parent.children.lock();
            if let Some(pos) = children.iter().position(|&c| c == id) {
                children.swap_remove(pos);
            }
        }

        groups.remove(&id);
        Ok(())
    }

    /// 查找 cgroup
    pub fn find(&self, id: u64) -> Option<Arc<CgroupRq>> {
        self.groups.lock().get(&id).map(Arc::clone)
    }

    /// 将进程迁移到目标 cgroup
    pub fn migrate(&self, pid: Pid, target_id: u64) -> Result<(), Errno> {
        let target = self.find(target_id).ok_or(Errno::ENOENT)?;

        // 先从旧 cgroup 移除 (遍历查找)
        {
            let groups = self.groups.lock();
            for cg in groups.values() {
                let procs = cg.procs.lock();
                if procs.contains(&pid) {
                    drop(procs);
                    cg.detach_proc(pid);
                    break;
                }
            }
        }

        // 加入新 cgroup
        if !target.attach_proc(pid) {
            return Err(Errno::EAGAIN); // PID 限制
        }

        Ok(())
    }

    /// 获取进程所属 cgroup
    pub fn cgroup_of(&self, pid: Pid) -> Option<Arc<CgroupRq>> {
        let groups = self.groups.lock();
        for cg in groups.values() {
            if cg.procs.lock().contains(&pid) {
                return Some(Arc::clone(cg));
            }
        }
        None
    }
}

// ============================================================================
// Errno (与 syscall/types.rs 对齐)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
#[allow(clippy::upper_case_acronyms)]
pub enum Errno {
    EPERM = 1,
    ENOENT = 2,
    EAGAIN = 11,
    ENOMEM = 12,
    EBUSY = 16,
    EINVAL = 22,
}

// ============================================================================
// 全局 cgroup 子系统实例
// ============================================================================

use core::sync::atomic::AtomicBool;
use crate::kernel::framework::sync::once_lock::OnceLock;

/// 全局 cgroup 子系统
static CGROUP_SUBSYSTEM: OnceLock<CgroupSubsystem> = OnceLock::new();
static CGROUP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 初始化 cgroup 子系统
pub fn cgroup_init() {
    CGROUP_SUBSYSTEM.get_or_init(CgroupSubsystem::new);
    CGROUP_INITIALIZED.store(true, Ordering::Release);
    crate::klog_ffi!(klog_ffi_info, "[CGROUP] subsystem initialized (root cgroup id=0)");
}

/// 获取全局 cgroup 子系统引用
///
/// # Panics
///
/// 如果 cgroup_init() 未调用则 panic
pub fn cgroup_subsystem() -> &'static CgroupSubsystem {
    CGROUP_SUBSYSTEM.get().expect("cgroup subsystem not initialized")
}

/// cgroup 是否已初始化
pub fn cgroup_is_initialized() -> bool {
    CGROUP_INITIALIZED.load(Ordering::Acquire)
}

// ============================================================================
// 系统调用
// ============================================================================

/// sys_cgroup_create — 创建子 cgroup
///
/// `a0`: 父 cgroup ID (0 = root)
/// `a1`: 名称字符串指针
/// `a2`: 名称长度
///
/// 返回: 新 cgroup ID (>0) 或 -errno
#[no_mangle]
pub fn sys_cgroup_create(parent_id: u64, _name_ptr: u64, _name_len: u64) -> i64 {
    if !cgroup_is_initialized() {
        return -(Errno::EINVAL as i64);
    }

    // TODO: copy name from user space; 暂用默认名称
    let name = alloc::format!("cg_{}", parent_id);
    let id = cgroup_subsystem().create_cgroup(parent_id, &name);
    if id == 0 {
        return -(Errno::EINVAL as i64);
    }
    id as i64
}

/// sys_cgroup_destroy — 删除 cgroup
///
/// `a0`: cgroup ID
///
/// 返回: 0 成功, -errno 失败
#[no_mangle]
pub fn sys_cgroup_destroy(cg_id: u64) -> i64 {
    if !cgroup_is_initialized() {
        return -(Errno::EINVAL as i64);
    }
    match cgroup_subsystem().remove_cgroup(cg_id) {
        Ok(()) => 0,
        Err(e) => -(e as i64),
    }
}

/// sys_cgroup_attach — 将进程迁移到 cgroup
///
/// `a0`: 目标 cgroup ID
/// `a1`: 进程 PID
///
/// 返回: 0 成功, -errno 失败
#[no_mangle]
pub fn sys_cgroup_attach(cg_id: u64, pid: u64) -> i64 {
    if !cgroup_is_initialized() {
        return -(Errno::EINVAL as i64);
    }
    match cgroup_subsystem().migrate(pid as Pid, cg_id) {
        Ok(()) => 0,
        Err(e) => -(e as i64),
    }
}

/// sys_cgroup_set_limit — 设置 cgroup 资源限制
///
/// `a0`: cgroup ID
/// `a1`: 控制器类型 (0=cpu_quota, 1=cpu_period, 2=memory_limit, 3=pids_max)
/// `a2`: 限制值
///
/// 返回: 0 成功, -errno 失败
#[no_mangle]
pub fn sys_cgroup_set_limit(cg_id: u64, controller: u64, value: u64) -> i64 {
    if !cgroup_is_initialized() {
        return -(Errno::EINVAL as i64);
    }

    let cg = match cgroup_subsystem().find(cg_id) {
        Some(c) => c,
        None => return -(Errno::ENOENT as i64),
    };

    match controller {
        0 => cg.cpu.set_quota(value),
        1 => cg.cpu.set_period(value),
        2 => cg.memory.set_limit(value),
        3 => cg.pids.set_max(value),
        _ => return -(Errno::EINVAL as i64),
    }

    0
}

/// sys_cgroup_get_stat — 获取 cgroup 统计信息
///
/// `a0`: cgroup ID
/// `a1`: 统计类型 (0=cpu_usage, 1=memory_usage, 2=pids_current, 3=io_read_bytes)
///
/// 返回: 统计值 或 -errno
#[no_mangle]
pub fn sys_cgroup_get_stat(cg_id: u64, stat_type: u64) -> i64 {
    if !cgroup_is_initialized() {
        return -(Errno::EINVAL as i64);
    }

    let cg = match cgroup_subsystem().find(cg_id) {
        Some(c) => c,
        None => return -(Errno::ENOENT as i64),
    };

    match stat_type {
        0 => cg.cpu.runtime_used.load(Ordering::Acquire) as i64,
        1 => cg.memory.usage_in_bytes.load(Ordering::Acquire) as i64,
        2 => cg.pids.current.load(Ordering::Acquire) as i64,
        3 => cg.io.stat_read_bytes.load(Ordering::Acquire) as i64,
        4 => cg.io.stat_write_bytes.load(Ordering::Acquire) as i64,
        5 => cg.cpu.nr_throttled.load(Ordering::Acquire) as i64,
        6 => cg.memory.max_usage_in_bytes.load(Ordering::Acquire) as i64,
        7 => cg.pids.events_fork_fail.load(Ordering::Acquire) as i64,
        _ => -(Errno::EINVAL as i64),
    }
}
