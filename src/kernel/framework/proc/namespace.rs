//! Linux 兼容 Namespace 框架 (D1)
//!
//! 提供 PID / Net / Mount / User / IPC / UTS 六种命名空间隔离,
//! 支持 unshare / setns / clone(CLONE_NEW*) 语义.
//!
//! ## 架构
//!
//! ```text
//! services/proc/namespace.rs (safe 代理)
//!     │
//!     ▼
//! framework/proc/namespace.rs (本文件, TCB)
//!     │
//!     ├── Process::namespaces (NamespaceSet)
//!     ├── fork: 拷贝或共享各 ns
//!     ├── clone: CLONE_NEW* 创建新 ns
//!     └── unshare/setns: 运行时切换
//! ```
//!
//! ## 设计原则
//!
//! 1. **引用计数**: 每个 ns 实例用 Arc 引用计数, fork 默认共享,
//!    CLONE_NEW* 创建新实例.
//! 2. **层级关系**: PID namespace 支持嵌套 (parent_ns),
//!    User namespace 支持嵌套 (owner_ns).
//! 3. **渐进集成**: 初期各 ns 为空壳数据结构, 后续逐步接入
//!    PID 分配 / VFS mount / 网络栈 / IPC 等子系统.

use core::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering};

use alloc::sync::Arc;
use alloc::vec::Vec;

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 常量
// ============================================================================

/// CLONE_NEW* 标志位 (与 Linux 一致)
pub const CLONE_NEWNS: u64 = 0x00020000;      // Mount namespace
pub const CLONE_NEWUTS: u64 = 0x04000000;     // UTS namespace
pub const CLONE_NEWIPC: u64 = 0x08000000;     // IPC namespace
pub const CLONE_NEWUSER: u64 = 0x10000000;    // User namespace
pub const CLONE_NEWPID: u64 = 0x20000000;     // PID namespace
pub const CLONE_NEWNET: u64 = 0x40000000;     // Network namespace
pub const CLONE_NEWCGROUP: u64 = 0x02000000;  // Cgroup namespace

/// 所有 CLONE_NEW* 掩码
pub const CLONE_NEW_ALL: u64 =
    CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWUSER | CLONE_NEWPID | CLONE_NEWNET | CLONE_NEWCGROUP;

// ============================================================================
// 命名空间类型枚举
// ============================================================================

/// 命名空间类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NsType {
    Mount = 0,
    Uts = 1,
    Ipc = 2,
    User = 3,
    Pid = 4,
    Net = 5,
    Cgroup = 6,
}

impl NsType {
    /// 从 CLONE_NEW* 标志位推断 NsType
    pub fn from_clone_flag(flag: u64) -> Option<Self> {
        match flag {
            CLONE_NEWNS => Some(Self::Mount),
            CLONE_NEWUTS => Some(Self::Uts),
            CLONE_NEWIPC => Some(Self::Ipc),
            CLONE_NEWUSER => Some(Self::User),
            CLONE_NEWPID => Some(Self::Pid),
            CLONE_NEWNET => Some(Self::Net),
            CLONE_NEWCGROUP => Some(Self::Cgroup),
            _ => None,
        }
    }

    /// 获取对应的 CLONE_NEW* 标志位
    pub fn to_clone_flag(self) -> u64 {
        match self {
            Self::Mount => CLONE_NEWNS,
            Self::Uts => CLONE_NEWUTS,
            Self::Ipc => CLONE_NEWIPC,
            Self::User => CLONE_NEWUSER,
            Self::Pid => CLONE_NEWPID,
            Self::Net => CLONE_NEWNET,
            Self::Cgroup => CLONE_NEWCGROUP,
        }
    }
}

// ============================================================================
// 全局命名空间 ID 分配
// ============================================================================

static NEXT_NS_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_ns_id() -> u64 {
    NEXT_NS_ID.fetch_add(1, Ordering::SeqCst)
}

// ============================================================================
// UTS Namespace
// ============================================================================

/// UTS Namespace — 主机名与域名隔离
///
/// Linux 中 UTS namespace 隔离 `nodename` 和 `domainname` 两个 utsname 字段.
#[derive(Debug)]
pub struct UtsNamespace {
    /// 命名空间全局 ID (用于 /proc/pid/ns/uts)
    pub id: u64,
    /// 主机名 (Linux: nodename, 最长 64 字节)
    pub nodename: IrqSpinLock<[u8; 65]>,
    /// 域名 (Linux: domainname, 最长 64 字节)
    pub domainname: IrqSpinLock<[u8; 65]>,
}

impl UtsNamespace {
    /// 创建新的 UTS namespace (默认继承 init 主机名)
    pub fn new() -> Self {
        let mut nodename = [0u8; 65];
        let default_name = b"QueenX";
        nodename[..default_name.len()].copy_from_slice(default_name);
        Self {
            id: alloc_ns_id(),
            nodename: IrqSpinLock::new(nodename),
            domainname: IrqSpinLock::new([0u8; 65]),
        }
    }

    /// 从父 namespace 继承 (共享引用, 不拷贝)
    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // UTS namespace 默认共享 (Linux 语义: fork 不创建新 UTS ns)
        Arc::clone(parent)
    }

    /// 创建新的 UTS namespace (CLONE_NEWUTS / unshare)
    pub fn new_from(parent: &Arc<Self>) -> Arc<Self> {
        let new = Self::new();
        // 拷贝父 namespace 的主机名和域名
        {
            let parent_node = parent.nodename.lock();
            let mut child_node = new.nodename.lock();
            child_node.copy_from_slice(&*parent_node);
        }
        {
            let parent_domain = parent.domainname.lock();
            let mut child_domain = new.domainname.lock();
            child_domain.copy_from_slice(&*parent_domain);
        }
        Arc::new(new)
    }

    /// 设置主机名
    pub fn set_nodename(&self, name: &[u8]) {
        let mut buf = self.nodename.lock();
        let len = name.len().min(64);
        buf[..len].copy_from_slice(&name[..len]);
        buf[len] = 0;
    }

    /// 获取主机名
    pub fn get_nodename(&self) -> alloc::string::String {
        let buf = self.nodename.lock();
        let end = buf.iter().position(|&b| b == 0).unwrap_or(64);
        alloc::string::String::from_utf8_lossy(&buf[..end]).into_owned()
    }
}

// ============================================================================
// IPC Namespace
// ============================================================================

/// IPC Namespace — System V IPC 与 POSIX 消息队列隔离
///
/// 隔离对象: 消息队列、信号量、共享内存段.
/// 当前封装 DynIpcNamespace 的引用.
#[derive(Debug)]
pub struct IpcNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// 消息队列数量限制
    pub msg_max: AtomicU32,
    /// 共享内存段数量限制
    pub shm_max: AtomicU32,
    /// 信号量数量限制
    pub sem_max: AtomicU32,
}

impl IpcNamespace {
    pub fn new() -> Self {
        Self {
            id: alloc_ns_id(),
            msg_max: AtomicU32::new(32),
            shm_max: AtomicU32::new(16),
            sem_max: AtomicU32::new(64),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // IPC namespace 默认共享
        Arc::clone(parent)
    }

    pub fn new_from(_parent: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::new())
    }
}

// ============================================================================
// PID Namespace
// ============================================================================

/// PID Namespace — 进程号隔离
///
/// 每个 PID namespace 有独立的 PID 分配空间.
/// 子 namespace 中的 PID 1 是该 namespace 的 init 进程.
#[derive(Debug)]
pub struct PidNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// 父 PID namespace (None = 根 namespace)
    pub parent: Option<Arc<PidNamespace>>,
    /// 当前 namespace 内的下一个 PID
    next_pid: AtomicU32,
    /// 当前 namespace 内的进程数量
    nr_processes: AtomicU32,
    /// 该 namespace 中最近重启的子 namespace 的偏移 (用于 PID 回收)
    level: AtomicU32,
}

impl PidNamespace {
    /// 创建根 PID namespace
    pub fn new_root() -> Self {
        Self {
            id: alloc_ns_id(),
            parent: None,
            next_pid: AtomicU32::new(1),
            nr_processes: AtomicU32::new(0),
            level: AtomicU32::new(0),
        }
    }

    /// 创建子 PID namespace
    pub fn new_child(parent: &Arc<Self>) -> Self {
        let parent_level = parent.level.load(Ordering::SeqCst);
        Self {
            id: alloc_ns_id(),
            parent: Some(Arc::clone(parent)),
            next_pid: AtomicU32::new(1),
            nr_processes: AtomicU32::new(0),
            level: AtomicU32::new(parent_level + 1),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // PID namespace 默认共享 (CLONE_NEWPID 才创建新的)
        Arc::clone(parent)
    }

    pub fn new_from(parent: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::new_child(parent))
    }

    /// 在该 namespace 内分配一个 PID
    pub fn alloc_pid(&self) -> u32 {
        self.nr_processes.fetch_add(1, Ordering::SeqCst);
        self.next_pid.fetch_add(1, Ordering::SeqCst)
    }

    /// 获取嵌套层级 (0 = 根)
    pub fn level(&self) -> u32 {
        self.level.load(Ordering::SeqCst)
    }
}

// ============================================================================
// Mount Namespace
// ============================================================================

/// Mount Namespace — 文件系统挂载点隔离
///
/// 每个 mount namespace 拥有独立的挂载点列表.
/// 后续与 VFS VfsManager 集成.
#[derive(Debug)]
pub struct MountNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// 挂载点数量
    pub mount_count: AtomicU32,
    /// 根文件系统路径
    pub root: IrqSpinLock<alloc::string::String>,
}

impl MountNamespace {
    pub fn new() -> Self {
        Self {
            id: alloc_ns_id(),
            mount_count: AtomicU32::new(0),
            root: IrqSpinLock::new(alloc::string::String::from("/")),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // Mount namespace: fork 默认共享, CLONE_NEWNS 创建新实例
        // 但 Linux 语义: CLONE_FS 未设置时 fork 共享, CLONE_FS 设置时不共享
        // 简化: 默认共享
        Arc::clone(parent)
    }

    pub fn new_from(parent: &Arc<Self>) -> Arc<Self> {
        let new = Self {
            id: alloc_ns_id(),
            mount_count: AtomicU32::new(parent.mount_count.load(Ordering::SeqCst)),
            root: IrqSpinLock::new(parent.root.lock().clone()),
        };
        Arc::new(new)
    }
}

// ============================================================================
// User Namespace
// ============================================================================

/// User Namespace — 用户/组 ID 隔离
///
/// User namespace 允许在 namespace 内拥有 UID 0 (root),
/// 而在父 namespace 中是普通用户.
/// 这是其他 namespace 的安全基础: 创建新 namespace
/// (除 User) 需要 CAP_SYS_ADMIN, 但 CLONE_NEWUSER 不需要.
#[derive(Debug)]
pub struct UserNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// 父 User namespace (None = 根 namespace)
    pub parent: Option<Arc<UserNamespace>>,
    /// 该 namespace 的拥有者 User namespace
    pub owner: Option<Arc<UserNamespace>>,
    /// namespace 内 root 的映射: (inner_start, outer_start, count)
    pub uid_map: IrqSpinLock<Option<(u32, u32, u32)>>,
    /// namespace 内 root group 的映射
    pub gid_map: IrqSpinLock<Option<(u32, u32, u32)>>,
    /// 嵌套层级
    level: AtomicU32,
}

impl UserNamespace {
    /// 创建根 User namespace
    pub fn new_root() -> Self {
        Self {
            id: alloc_ns_id(),
            parent: None,
            owner: None,
            uid_map: IrqSpinLock::new(Some((0, 0, 65536))), // 默认 1:1 映射
            gid_map: IrqSpinLock::new(Some((0, 0, 65536))),
            level: AtomicU32::new(0),
        }
    }

    /// 创建子 User namespace
    pub fn new_child(parent: &Arc<Self>) -> Self {
        let parent_level = parent.level.load(Ordering::SeqCst);
        Self {
            id: alloc_ns_id(),
            parent: Some(Arc::clone(parent)),
            owner: Some(Arc::clone(parent)),
            uid_map: IrqSpinLock::new(None), // 新 namespace 需要显式写 uid_map
            gid_map: IrqSpinLock::new(None),
            level: AtomicU32::new(parent_level + 1),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // User namespace 默认共享
        Arc::clone(parent)
    }

    pub fn new_from(parent: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::new_child(parent))
    }

    /// 获取嵌套层级
    pub fn level(&self) -> u32 {
        self.level.load(Ordering::SeqCst)
    }

    /// 在该 namespace 内将 UID 映射到父 namespace
    pub fn map_uid(&self, inner_uid: u32) -> u32 {
        let map = self.uid_map.lock();
        match *map {
            Some((inner_start, outer_start, count)) => {
                if inner_uid >= inner_start && inner_uid < inner_start + count {
                    outer_start + (inner_uid - inner_start)
                } else {
                    // 超出映射范围的 UID 映射到 nobody (65534)
                    65534
                }
            }
            None => 65534, // 无映射 → nobody
        }
    }

    /// 在该 namespace 内将 GID 映射到父 namespace
    pub fn map_gid(&self, inner_gid: u32) -> u32 {
        let map = self.gid_map.lock();
        match *map {
            Some((inner_start, outer_start, count)) => {
                if inner_gid >= inner_start && inner_gid < inner_start + count {
                    outer_start + (inner_gid - inner_start)
                } else {
                    65534
                }
            }
            None => 65534,
        }
    }
}

// ============================================================================
// Network Namespace
// ============================================================================

/// Network Namespace — 网络栈隔离
///
/// 每个 net namespace 拥有独立的:
/// - 网络设备列表
/// - IP 地址 / 路由表
/// - iptables / netfilter 规则
/// - 端口号空间
#[derive(Debug)]
pub struct NetNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// 是否为 init namespace
    pub is_init: bool,
    /// 本 namespace 的回环设备是否已启用
    pub loopback_up: AtomicU32,
    /// 端口分配偏移 (每个 namespace 独立)
    pub next_ephemeral_port: AtomicU16,
}

impl NetNamespace {
    /// 创建 init (根) network namespace
    pub fn new_init() -> Self {
        Self {
            id: alloc_ns_id(),
            is_init: true,
            loopback_up: AtomicU32::new(0),
            next_ephemeral_port: AtomicU16::new(32768),
        }
    }

    /// 创建新的 network namespace
    pub fn new() -> Self {
        Self {
            id: alloc_ns_id(),
            is_init: false,
            loopback_up: AtomicU32::new(0),
            next_ephemeral_port: AtomicU16::new(32768),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        // Net namespace 默认共享
        Arc::clone(parent)
    }

    pub fn new_from(_parent: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::new())
    }
}

// ============================================================================
// Cgroup Namespace
// ============================================================================

/// Cgroup Namespace — cgroup 根路径隔离
///
/// 隔离 /proc/self/cgroup 中看到的 cgroup 路径.
#[derive(Debug)]
pub struct CgroupNamespace {
    /// 命名空间全局 ID
    pub id: u64,
    /// cgroup 根路径
    pub root_path: IrqSpinLock<alloc::string::String>,
}

impl CgroupNamespace {
    pub fn new() -> Self {
        Self {
            id: alloc_ns_id(),
            root_path: IrqSpinLock::new(alloc::string::String::from("/")),
        }
    }

    pub fn fork_from(parent: &Arc<Self>) -> Arc<Self> {
        Arc::clone(parent)
    }

    pub fn new_from(parent: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self {
            id: alloc_ns_id(),
            root_path: IrqSpinLock::new(parent.root_path.lock().clone()),
        })
    }
}

// ============================================================================
// NamespaceSet — 进程的命名空间集合
// ============================================================================

/// 进程的命名空间集合
///
/// 每个 Process 持有一个 NamespaceSet, 包含所有 6 种 namespace 的 Arc 引用.
/// fork 默认共享 (Arc::clone), CLONE_NEW* 创建新实例.
#[derive(Debug)]
pub struct NamespaceSet {
    pub uts: Arc<UtsNamespace>,
    pub ipc: Arc<IpcNamespace>,
    pub pid: Arc<PidNamespace>,
    pub mount: Arc<MountNamespace>,
    pub user: Arc<UserNamespace>,
    pub net: Arc<NetNamespace>,
    pub cgroup: Arc<CgroupNamespace>,
}

impl NamespaceSet {
    /// 创建 init 进程的 namespace 集合 (根 namespace)
    pub fn new_init() -> Self {
        Self {
            uts: Arc::new(UtsNamespace::new()),
            ipc: Arc::new(IpcNamespace::new()),
            pid: Arc::new(PidNamespace::new_root()),
            mount: Arc::new(MountNamespace::new()),
            user: Arc::new(UserNamespace::new_root()),
            net: Arc::new(NetNamespace::new_init()),
            cgroup: Arc::new(CgroupNamespace::new()),
        }
    }

    /// fork 继承 (默认共享所有 namespace)
    pub fn fork_from(parent: &NamespaceSet) -> Self {
        Self {
            uts: UtsNamespace::fork_from(&parent.uts),
            ipc: IpcNamespace::fork_from(&parent.ipc),
            pid: PidNamespace::fork_from(&parent.pid),
            mount: MountNamespace::fork_from(&parent.mount),
            user: UserNamespace::fork_from(&parent.user),
            net: NetNamespace::fork_from(&parent.net),
            cgroup: CgroupNamespace::fork_from(&parent.cgroup),
        }
    }

    /// 根据 clone_flags 创建新 namespace
    ///
    /// flags 中的 CLONE_NEW* 位决定哪些 namespace 需要创建新实例,
    /// 其余继承父进程.
    pub fn clone_from(parent: &NamespaceSet, flags: u64) -> Self {
        let new_ns_flags = flags & CLONE_NEW_ALL;

        // CLONE_NEWUSER 必须最先处理 (其他新 ns 的 owner 依赖它)
        let user = if new_ns_flags & CLONE_NEWUSER != 0 {
            UserNamespace::new_from(&parent.user)
        } else {
            UserNamespace::fork_from(&parent.user)
        };

        let uts = if new_ns_flags & CLONE_NEWUTS != 0 {
            UtsNamespace::new_from(&parent.uts)
        } else {
            UtsNamespace::fork_from(&parent.uts)
        };

        let ipc = if new_ns_flags & CLONE_NEWIPC != 0 {
            IpcNamespace::new_from(&parent.ipc)
        } else {
            IpcNamespace::fork_from(&parent.ipc)
        };

        let pid = if new_ns_flags & CLONE_NEWPID != 0 {
            PidNamespace::new_from(&parent.pid)
        } else {
            PidNamespace::fork_from(&parent.pid)
        };

        let mount = if new_ns_flags & CLONE_NEWNS != 0 {
            MountNamespace::new_from(&parent.mount)
        } else {
            MountNamespace::fork_from(&parent.mount)
        };

        let net = if new_ns_flags & CLONE_NEWNET != 0 {
            NetNamespace::new_from(&parent.net)
        } else {
            NetNamespace::fork_from(&parent.net)
        };

        let cgroup = if new_ns_flags & CLONE_NEWCGROUP != 0 {
            CgroupNamespace::new_from(&parent.cgroup)
        } else {
            CgroupNamespace::fork_from(&parent.cgroup)
        };

        Self {
            uts,
            ipc,
            pid,
            mount,
            user,
            net,
            cgroup,
        }
    }

    /// unshare: 对指定 flags 中的 namespace 创建新实例
    pub fn unshare(&mut self, flags: u64) -> Result<(), Errno> {
        let new_ns_flags = flags & CLONE_NEW_ALL;
        if new_ns_flags == 0 {
            return Err(Errno::EINVAL);
        }

        // CLONE_NEWUSER 先处理
        if new_ns_flags & CLONE_NEWUSER != 0 {
            self.user = UserNamespace::new_from(&self.user);
        }
        if new_ns_flags & CLONE_NEWUTS != 0 {
            self.uts = UtsNamespace::new_from(&self.uts);
        }
        if new_ns_flags & CLONE_NEWIPC != 0 {
            self.ipc = IpcNamespace::new_from(&self.ipc);
        }
        if new_ns_flags & CLONE_NEWPID != 0 {
            // PID namespace: unshare 只标记, 实际生效在下一个 fork
            // Linux 语义: unshare(CLONE_NEWPID) 不改变当前进程的 PID namespace,
            // 而是设置子进程将使用的新 PID namespace.
            // 此处简化: 直接创建新实例
            self.pid = PidNamespace::new_from(&self.pid);
        }
        if new_ns_flags & CLONE_NEWNS != 0 {
            self.mount = MountNamespace::new_from(&self.mount);
        }
        if new_ns_flags & CLONE_NEWNET != 0 {
            self.net = NetNamespace::new_from(&self.net);
        }
        if new_ns_flags & CLONE_NEWCGROUP != 0 {
            self.cgroup = CgroupNamespace::new_from(&self.cgroup);
        }

        Ok(())
    }

    /// setns: 切换到目标 namespace
    ///
    /// `ns_type` 指定要切换的 namespace 类型,
    /// `target` 是目标 namespace 的 ID.
    pub fn setns_by_type(&mut self, ns_type: NsType, target_id: u64) -> Result<(), Errno> {
        let registry = NS_REGISTRY.lock();
        let entry = match registry.find(target_id) {
            Some(e) => e,
            None => return Err(Errno::EINVAL),
        };

        match ns_type {
            NsType::Uts => {
                if let Some(ref ns) = entry.uts { self.uts = Arc::clone(ns); }
            }
            NsType::Ipc => {
                if let Some(ref ns) = entry.ipc { self.ipc = Arc::clone(ns); }
            }
            NsType::Pid => {
                if let Some(ref ns) = entry.pid { self.pid = Arc::clone(ns); }
            }
            NsType::Mount => {
                if let Some(ref ns) = entry.mount { self.mount = Arc::clone(ns); }
            }
            NsType::User => {
                if let Some(ref ns) = entry.user { self.user = Arc::clone(ns); }
            }
            NsType::Net => {
                if let Some(ref ns) = entry.net { self.net = Arc::clone(ns); }
            }
            NsType::Cgroup => {
                if let Some(ref ns) = entry.cgroup { self.cgroup = Arc::clone(ns); }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Namespace 注册表 (用于 setns 查找)
// ============================================================================

/// 注册表中的 namespace 条目 (存储 Arc 引用)
pub struct NsRegistryEntry {
    pub id: u64,
    pub uts: Option<Arc<UtsNamespace>>,
    pub ipc: Option<Arc<IpcNamespace>>,
    pub pid: Option<Arc<PidNamespace>>,
    pub mount: Option<Arc<MountNamespace>>,
    pub user: Option<Arc<UserNamespace>>,
    pub net: Option<Arc<NetNamespace>>,
    pub cgroup: Option<Arc<CgroupNamespace>>,
}

/// 全局 namespace 注册表
pub struct NsRegistry {
    entries: Vec<NsRegistryEntry>,
}

impl NsRegistry {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// 注册一个 namespace 集合 (用于 setns 查找)
    pub fn register(&mut self, ns_set: &NamespaceSet) {
        self.entries.push(NsRegistryEntry {
            id: ns_set.uts.id, // 用 UTS ID 作为集合 ID (简化)
            uts: Some(Arc::clone(&ns_set.uts)),
            ipc: Some(Arc::clone(&ns_set.ipc)),
            pid: Some(Arc::clone(&ns_set.pid)),
            mount: Some(Arc::clone(&ns_set.mount)),
            user: Some(Arc::clone(&ns_set.user)),
            net: Some(Arc::clone(&ns_set.net)),
            cgroup: Some(Arc::clone(&ns_set.cgroup)),
        });
    }

    /// 按 ID 查找
    pub fn find(&self, id: u64) -> Option<&NsRegistryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

/// 全局 namespace 注册表实例
static NS_REGISTRY: IrqSpinLock<NsRegistry> = IrqSpinLock::new(NsRegistry::new());

/// 注册 namespace 集合到全局注册表
pub fn ns_register(ns_set: &NamespaceSet) {
    NS_REGISTRY.lock().register(ns_set);
}

// ============================================================================
// Syscall 接口
// ============================================================================

/// sys_unshare — 取消共享指定 namespace
///
/// # 参数
/// - a0: flags (CLONE_NEW* 位掩码)
pub fn sys_unshare(flags: u64) -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();

    let result = crate::kernel::framework::proc::process::PROCESS_TABLE
        .with_process_mut(pid, |p| {
            p.namespaces.lock().unshare(flags)
        });

    match result {
        Some(Ok(())) => 0,
        Some(Err(e)) => -(e as i64),
        None => -(Errno::ESRCH as i64),
    }
}

/// sys_setns — 切换到指定 namespace
///
/// # 参数
/// - a0: ns_type (NsType 枚举值)
/// - a1: target_ns_id (目标 namespace 的全局 ID)
pub fn sys_setns(ns_type: u64, target_ns_id: u64) -> i64 {
    let ns_t = match NsType::from_clone_flag(1 << (ns_type + 8)) {
        // 简化: ns_type 直接传 NsType 枚举值
        Some(t) => t,
        None => {
            // 尝试直接作为 NsType 枚举值
            match ns_type {
                0 => NsType::Mount,
                1 => NsType::Uts,
                2 => NsType::Ipc,
                3 => NsType::User,
                4 => NsType::Pid,
                5 => NsType::Net,
                6 => NsType::Cgroup,
                _ => return -(Errno::EINVAL as i64),
            }
        }
    };

    let pid = crate::kernel::framework::proc::api::process_get_current_pid();

    let result = crate::kernel::framework::proc::process::PROCESS_TABLE
        .with_process_mut(pid, |p| {
            p.namespaces.lock().setns_by_type(ns_t, target_ns_id)
        });

    match result {
        Some(Ok(())) => 0,
        Some(Err(e)) => -(e as i64),
        None => -(Errno::ESRCH as i64),
    }
}
