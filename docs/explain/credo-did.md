# Credo / DID 安全模型 — 开发规划设计

> **版本**: 0.1.0-draft  
> **作者**: AntX Kernel Team  
> **日期**: 2025-06  
> **关联文档**: [posix-interface.md](../plan/posix-interface.md), [kernel-architecture.md](kernel-architecture.md)

---

## 1. 概述与目标

### 1.1 背景

AntX 内核当前的安全子系统命名为 PWM（Password-determined identity Model）。经过架构评审，决定将其重构为 **Credo**（身份凭证系统）+ **DID**（Domain Identity，域身份）双层模型。此次重构解决以下问题：

| 问题 | 当前状态 | 目标状态 |
|------|---------|---------|
| 命名冲突 | `pwm` 与 Pulse Width Modulation 歧义，嵌入式领域混乱 | `credo` 零歧义 |
| 权限模型不完整 | 仅能力检查，忽略 POSIX rwx 模式位 | 双路径：POSIX DAC + 能力并查 |
| 身份信息分散 | `PwmId` + `posix_uid` + `posix_gid` 各自独立传递 | 统一为 `DomainId` 结构体 |
| 能力粒度粗糙 | `CapBits(0)..CapBits(u64::MAX)` 无语义细分 | 保留 16 域 × 64bit，语义不变 |

### 1.2 核心概念

```
credo
 ├── DID (Domain Identity)     ← 域身份，承载 uid/gid + 能力矩阵 + 特权级别
 ├── Identity Table            ← 身份注册表 (SHA-256 密码派生)
 ├── Session Manager           ← 登录/登出会话管理
 ├── Capability Engine         ← 能力检查引擎
 ├── First Token               ← 首次引导的身份种子
 └── Audit Log                 ← 安全审计日志
```

### 1.3 设计原则

1. **接口严格 POSIX 兼容** — 系统调用签名、返回值、错误码不变
2. **DID 对用户态隐形** — 用户程序完全感知不到 DID 的存在
3. **能力授予显式化** — 不存在 setuid、不存在隐式继承
4. **一步到位的架构** — 不设过渡期、不留兼容性 shim

---

## 2. DID 数据结构设计

### 2.1 DomainId 定义

```rust
/// 域身份 — 每个 syscall 执行时的完整安全上下文
///
/// 生命周期: 在 syscall 入口解析，函数参数传递，syscall 返回时销毁。
/// 不跨调用存活，不存储在任何全局状态中。
#[derive(Debug, Clone)]
pub struct DomainId {
    /// POSIX 用户 ID (0 表示域内超级用户)
    pub uid: u32,
    /// POSIX 组 ID
    pub gid: u32,
    /// 域间特权级别: 0 (全能者) ~ 255 (受限者)
    pub privilege_level: u8,
    /// 域行为标志
    pub flags: DomainFlags,
    /// 16 功能域 × 64bit 能力矩阵
    pub caps: [CapBits; 16],
}

bitflags::bitflags! {
    pub struct DomainFlags: u8 {
        /// 默认: POSIX DAC 或能力任一通过即放行
        const HYBRID      = 0;
        /// 仅 POSIX rwx (传统程序兼容模式)
        const POSIX_ONLY  = 1;
        /// 仅能力检查 (纯 credo 模式)
        const CAP_ONLY    = 2;
        /// 标记为系统域 (privilege_level=0 等价于此)
        const SYSTEM      = 4;
    }
}
```

### 2.2 与当前 PwmEntry 的映射

```
当前 PwmEntry:                              DID 对应:
  pwm: u64           → SHA-256 身份标识     (不暴露给 DID 调用方)
  posix_uid: u32     → DomainId.uid
  posix_gid: u32     → DomainId.gid
  privilege_level: u8 → DomainId.privilege_level
  caps: [u64; 16]    → DomainId.caps
  flags: u16         → DomainId.flags (仅保留域行为位)
  creator_pwm: u64   → (仅用于信任链审计，不出现在 DID 中)
  password_hash: [u8] → (仅在登录时使用，不出现在 DID 中)
```

### 2.3 fallback DID

```rust
impl DomainId {
    /// 无登录会话时的回退 DID (相当于"静默 root")
    pub const fn fallback() -> Self {
        Self {
            uid: 1000,
            gid: 1000,
            privilege_level: 0,
            flags: DomainFlags::HYBRID,
            caps: [CapBits::ALL; 16],
        }
    }
}
```

### 2.4 权限检查入口

```rust
impl DomainId {
    /// POSIX DAC: 检查 uid/gid 对文件 mode 位的访问权限
    pub fn check_posix_access(
        &self,
        file_owner_uid: u32,
        file_group_gid: u32,
        file_mode: u16,
        desired_access: u16,
    ) -> bool;

    /// 完全文件权限检查 (POSIX + 能力双路径)
    pub fn check_fs_access(
        &self,
        file_owner_uid: u32,
        file_group_gid: u32,
        file_mode: u16,
        file_owner_pwm: u64,
        desired_cap: CapBits,
    ) -> bool;

    /// 检查特定域的能力
    pub fn has_cap(&self, domain: CapDomain, cap: CapBits) -> bool;
}
```

---

## 3. Credo 子系统架构

### 3.1 模块目录结构

```
src/kernel/credo/
├── mod.rs              ← 模块入口，公共 re-export
├── types.rs            ← DomainId, CapDomain, CapBits, CredoError 等基础类型
├── identity.rs         ← 身份表 (256 槽，SHA-256 密码派生，CRUD)
├── session.rs          ← 会话管理 (per-CPU login/logout)
├── engine.rs           ← 能力检查引擎 (POSIX DAC + Capability 双路径)
├── bootstrap.rs        ← First Token / 首次引导逻辑
├── grant.rs            ← 能力授予/撤销 (grant/revoke/transfer)
├── audit.rs            ← 审计日志 (环形缓冲，持久化到 HvFS)
├── storage.rs          ← 身份序列化/反序列化到磁盘
└── ffi.rs              ← C 兼容导出接口 (供 C 代码调用)
```

### 3.2 各模块职责

| 模块 | 职责 | 关键函数 |
|------|------|---------|
| `types.rs` | DID、CapDomain(16域)、CapBits(64bit)、DomainFlags、错误类型 | 数据结构定义 |
| `identity.rs` | 密码→SHA-256→身份派生，256 槽表管理 | `create()`, `find()`, `delete()`, `verify_password()` |
| `session.rs` | per-CPU 登录会话，`current_did()` 解析 | `login()`, `logout()`, `current_did()`, `get_current_uid()` |
| `engine.rs` | POSIX DAC + 能力双路径检查 | `check_fs_access()`, `check_net_access()`, `check_proc_access()`, `has_cap()` |
| `bootstrap.rs` | 首次引导身份创建，First Token 种子 | `try_genesis()` |
| `grant.rs` | 身份间能力授予/撤销/转移 | `grant()`, `revoke()`, `transfer_creator()` |
| `audit.rs` | 安全事件日志，append-only 缓冲 | `log()`, `flush()` |
| `storage.rs` | 身份表持久化 | `save_database()`, `load_database()` |
| `ffi.rs` | C 兼容桥接 | `credo_init()`, `credo_get_current()` |

### 3.3 数据流

```
用户态
  │  open("/tmp/x", O_RDONLY)     ← 标准 POSIX
  │  int 0x80
  ▼
syscall 层 (mod.rs)
  │  sys_open(path, flags, mode)
  │  ① let did = credo::session::current_did();   ← DID 解析
  │  ② vfs_open(path, flags, &did)                ← DID 传递
  ▼
VFS 层 (vfs/ffi.rs)
  │  vfs_open_internal(path, flags, did)
  │  node = lookup(path)
  │  ③ did.check_fs_access(node.owner_uid, node.group_gid,
  │       node.mode, node.owner_pwm, FS_CAP_READ)
  │        ├── POSIX DAC 路径: uid==owner? mode & 0o400? → pass
  │        └── 能力路径:      did.has_cap(FS, FS_CAP_READ)? → pass
  │  ④ 实际执行文件读取
  ▼
存储层 (ramfs / HvFS)
```

### 3.4 CapDomain 16 域定义

```rust
pub const DOMAIN_SYSTEM:    CapDomain = CapDomain(0);   // 系统控制 (reboot, shutdown...)
pub const DOMAIN_FS:        CapDomain = CapDomain(1);   // 文件系统
pub const DOMAIN_NET:       CapDomain = CapDomain(2);   // 网络
pub const DOMAIN_PROC:      CapDomain = CapDomain(3);   // 进程管理
pub const DOMAIN_DEVICE:    CapDomain = CapDomain(4);   // 设备驱动
pub const DOMAIN_USER_MGMT: CapDomain = CapDomain(5);   // 用户/身份管理
pub const DOMAIN_IPC:       CapDomain = CapDomain(6);   // 进程间通信
pub const DOMAIN_MEM:       CapDomain = CapDomain(7);   // 内存管理
pub const DOMAIN_TIME:      CapDomain = CapDomain(8);   // 时间/定时器
pub const DOMAIN_BARRIER:   CapDomain = CapDomain(9);   // 故障恢复屏障
pub const DOMAIN_SIGNAL:    CapDomain = CapDomain(10);  // 信号
pub const DOMAIN_SHM:       CapDomain = CapDomain(11);  // 共享内存
pub const DOMAIN_SEM:       CapDomain = CapDomain(12);  // 信号量
pub const DOMAIN_MSGQ:      CapDomain = CapDomain(13);  // 消息队列
pub const DOMAIN_DMA:       CapDomain = CapDomain(14);  // DMA
pub const DOMAIN_RESERVED:  CapDomain = CapDomain(15);  // 保留域
```

### 3.5 Per-Domain CapBits 示例

```
CapDomain::FS (1):
  FS_CAP_READ    = 1 << 0
  FS_CAP_WRITE   = 1 << 1
  FS_CAP_CREATE  = 1 << 3
  FS_CAP_DELETE  = 1 << 4
  FS_CAP_CHOWN   = 1 << 5
  FS_CAP_CHMOD   = 1 << 6
  FS_CAP_EXECUTE = 1 << 7
  FS_CAP_MOUNT   = 1 << 8
  ...            (剩余 56 位预留)

CapDomain::NET (2):
  NET_CAP_BIND       = 1 << 0
  NET_CAP_CONNECT    = 1 << 1
  NET_CAP_LISTEN     = 1 << 2
  NET_CAP_RAW        = 1 << 3
  NET_CAP_ADMIN      = 1 << 4
  ...                (剩余 59 位预留)

CapDomain::PROC (3):
  PROC_CAP_FORK      = 1 << 0
  PROC_CAP_EXEC      = 1 << 1
  PROC_CAP_KILL      = 1 << 2
  PROC_CAP_WAIT      = 1 << 3
  PROC_CAP_PTRACE    = 1 << 4
  ...                (剩余 59 位预留)
```

---

## 4. POSIX 兼容性设计

### 4.1 双路径权限检查

每个子系统在权限判断点执行：

```
credential_check(did, resource, operation):
    if did.flags == CAP_ONLY:
        return did.has_cap(domain, cap)          # 纯能力模式

    if did.flags == POSIX_ONLY:
        return posix_dac_check(did, resource, op) # 纯 POSIX 模式

    # HYBRID (默认): 任一通过即放行
    return posix_dac_check(...) || did.has_cap(domain, cap)
```

### 4.2 POSIX syscall 兼容矩阵

| Syscall | DID 行为 | 兼容性 |
|---------|---------|--------|
| `getuid/getgid` | 返回 `did.uid / did.gid` | ✅ 等价 |
| `geteuid/getegid` | 返回 `did.uid / did.gid` | ✅ 等价 |
| `setuid/setgid` | 返回 `EPERM`（有意设计） | ⚠️ 有意的破坏 |
| `chmod/fchmod` | 更新文件 `mode` 字段 | ✅ 等价 |
| `chown/fchown` | 更新文件 `owner_uid/owner_pwm` | ✅ 等价，额外需 `FS_CAP_CHOWN` |
| `open/read/write` | POSIX+DAC 或能力双路径 | ✅ 优于当前 |
| `fork/execve` | 子进程继承父进程的 DID | ✅ 等价 |
| `setreuid/setregid` | 返回 `EPERM` | ⚠️ 有意的破坏 |

### 4.3 不受影响的子系统

以下子系统不接受 DID 参数，完全不感知 credo：

- 内存管理 (`mm/`) — 调度公平性无关身份
- 调度器 (`proc/scheduler*`) — 调度公平性无关身份
- 中断处理 (`idt/`) — 上下文无关身份
- 定时器 (`timer/`) — 设备无关身份
- DMA 引擎 (`dma/`) — 硬件无关身份

---

## 5. 实施计划

### Phase 1: 重命名 + DID 结构体（零行为变更）

**目标**: `pwm` → `credo`，插入 `DomainId` 但不改变任何权限判断逻辑。

**变更清单**:

| 步骤 | 操作 | 影响文件 |
|------|------|---------|
| 1.1 | 创建 `src/kernel/credo/` 目录结构 | 12 个新文件 |
| 1.2 | 定义 `DomainId` + `CapDomain` + `CapBits` + `DomainFlags` | `types.rs` |
| 1.3 | 迁移 `pwm/table.rs` → `credo/identity.rs` | 函数重命名，逻辑不变 |
| 1.4 | 迁移 `pwm/session.rs` → `credo/session.rs` | `pwm_get_current()` → `current_did()` |
| 1.5 | 迁移 `pwm/engine.rs` → `credo/engine.rs` | 接口改为接受 `&DomainId` |
| 1.6 | 迁移 `pwm/ffi.rs` → `credo/ffi.rs` | C 兼容接口保持 |
| 1.7 | 更新 `pwm::ffi::pwm_*` 全部调用点 | `syscall/mod.rs`, `fs/`, `proc/` |
| 1.8 | 删除 `src/kernel/pwm/` | 完整删除 |
| 1.9 | 更新 `kernel/mod.rs` 模块声明 | `pub mod credo` |
| 1.10 | 编译 + make test-unit 验证 | 自动化 |

**验证标准**: `make test-unit` → ALL 255 TESTS PASSED，零行为回归。

### Phase 2: POSIX DAC 路径插入

**目标**: 在 `check_permission` 中插入 POSIX rwx 检查，作为第一道防线。

**变更清单**:

| 步骤 | 操作 | 影响文件 |
|------|------|---------|
| 2.1 | `ramfs.check_permission` 增加 `DomainId` 参数，插入 POSIX DAC | `ramfs.rs` |
| 2.2 | `HvFS.check_permission` 同上 | `hvfs.rs` |
| 2.3 | `vfs_open/vfs_read/vfs_write` 接口从 `pwm: u64` 改为 `did: &DomainId` | `vfs/ffi.rs` |
| 2.4 | `syscall/mod.rs` 所有 `pwm_get_current()` 改为 `credo::current_did()` | `syscall/mod.rs` |
| 2.5 | 新增 `DomainId::check_posix_access()` 辅助方法 | `types.rs` |
| 2.6 | QEMU smoke test — 验证文件权限行为 | 手动测试 |

**验证标准**: 创建 mode=0o644 文件，非 owner 进程可读不可写。

### Phase 3: 审计 + 持久化

**目标**: 审计日志完善，身份表持久化路径梳理。

**变更清单**:

| 步骤 | 操作 | 影响文件 |
|------|------|---------|
| 3.1 | 审计日志增加 `FsAccess` / `CapGrant` / `CapRevoke` 事件类型 | `audit.rs` |
| 3.2 | `storage.rs` 重写序列化逻辑（移除对旧 PWM 格式的兼容） | `storage.rs` |
| 3.3 | `bootstrap.rs` First Token 简化（count>0 守卫替代 TSC nonce） | `bootstrap.rs` |
| 3.4 | 集成测试 — 验证身份持久化 + 重启恢复 | `make test-unit` |

### Phase 4: 文档 + 清理

| 步骤 | 操作 |
|------|------|
| 4.1 | 更新 `docs/explain/syscall.md` 安全模型章节 |
| 4.2 | 更新 `docs/explain/kernel-architecture.md` |
| 4.3 | 移除所有 `#[allow(dead_code)]` 的 PWM 残留注释 |
| 4.4 | `make test-host test-unit` 双验证 |
| 4.5 | git commit: "pwm → credo: Domain Identity security model" |

---

## 6. 关键设计决策记录

| 决策 | 选项 A | 选项 B | 采纳 | 理由 |
|------|--------|--------|------|------|
| DID 可变性 | DID 可以 `setuid()` | DID 不可变 | **B** | 身份来自密码推导，不应运行时篡改 |
| POSIX vs 能力优先级 | POSIX 优先 | 能力优先 | **A** | POSIX DAC 检查更宽松，不影响传统程序 |
| 双路径采用 | AND（两者都需通过） | OR（任一通过即放行） | **B** | 最大兼容性，目标是 +POSIX 不失去能力 |
| 能力粒度 | 16 域 × 64bit（当前） | 扩展域数量 | **A**（暂不变） | 16 域已足够，先解决结构问题 |
| session 作用域 | per-process | per-CPU | **B**（维持现状） | 简化实现，需要隔离用能力授予 |
| 首次引导 | 编译时常量 ROOT | First Token 动态派生 | **B** | 二进制中不应硬编码 root 身份 |

---

## 7. 风险与缓解

| 风险 | 概率 | 缓解 |
|------|------|------|
| `setuid` 破坏导致 Ported 程序失败 | 中 | Phase 2 增加 `DOMAIN_FLAG_POSIX_ONLY` 模式，允许 setuid |
| 权限检查双路径产生逻辑漏洞 | 低 | Phase 2 增加针对性测试用例 |
| 大规模重命名引入拼写错误 | 低 | Phase 1 用脚本自动化重命名 |
| 性能下降（per-syscall DID 构建） | 极低 | DID 约 152 bytes，栈分配，零堆分配 |

---

## 8. 命名规范

| 新名称 | 旧名称 | 说明 |
|--------|--------|------|
| `credo` | `pwm` | 模块名 |
| `DomainId` | - | 新增结构体 |
| `current_did()` | `pwm_get_current()` | 获取当前域身份 |
| `identity::table` | `pwm::table` | 身份注册表 |
| `session::login()` | `pwm_login()` | 登录 |
| `engine::check()` | `pwm_has_capability()` | 能力检查 |
| `bootstrap::try_genesis()` | `pwm_try_genesis()` | 首次引导 |
| `grant::grant()` | `pwm_grant()` | 能力授予 |

---

## 9. 附录

### 9.1 相关源代码文件

| 文件 | 变更类型 |
|------|---------|
| `src/kernel/pwm/*` | **删除** |
| `src/kernel/credo/*` | **新建** |
| `src/kernel/mod.rs` | 修改模块声明 |
| `src/kernel/syscall/mod.rs` | `pwm_get_current()` → `credo::current_did()` |
| `src/kernel/fs/vfs/ffi.rs` | 接口参数 `pwm: u64` → `did: &DomainId` |
| `src/kernel/fs/ramfs/ramfs.rs` | `check_permission` 增加 POSIX DAC |
| `src/kernel/fs/hvfs/hvfs.rs` | `check_permission` 增加 POSIX DAC |
| `src/kernel/proc/ffi.rs` | `pwm` 相关调用更新 |
| `src/rust/src/lib.rs` | 注释更新 |
| `src/kernel/tests/test_pwm.rs` | **重命名** → `test_credo.rs` |
| `src/user/lib/src/sys.rs` | syscall 编号不变，零变更 |
| `src/user/axsh/src/commands/identity.rs` | 函数名更新 |
| `docs/explain/syscall.md` | 安全模型章节更新 |
| `docs/explain/kernel-architecture.md` | 子系统名称更新 |

### 9.2 术语表

| 术语 | 定义 |
|------|------|
| **Credo** | AntX 内核的身份凭证子系统。管理身份派生、DID 解析、能力检查、会话、审计。 |
| **DID (Domain Identity)** | 域身份。每个 syscall 执行时的完整安全上下文：uid + gid + privilege_level + 16 域能力矩阵 + 域标志。 |
| **PwmId** | SHA-256(password + salt) 的 64-bit 派生值。DID 的内部实现细节，不暴露给调用方。 |
| **CapDomain** | 能力域。16 个功能分区 (FS/NET/PROC/DEVICE...)，每域 64 位能力。 |
| **CapBits** | 域内能力位。64 位位图，每位代表一个具体操作权限。 |
| **privilege_level** | 特权级别。0（全能者）~ 255（受限者）。仅对更低级别身份的创建才有意义。 |
| **First Token** | 首次引导时动态生成的"能力种子"。bootstrap() 创建首个身份并授予全部域的全部能力，消耗即灭。 |
| **HYBRID** | 默认域标志。POSIX DAC 或能力任一通过即放行。 |
| **POSIX_ONLY** | 域标志。仅走 POSIX rwx 路径，用于传统程序沙箱。 |
| **CAP_ONLY** | 域标志。仅走能力矩阵路径，用于 credo 原生程序。 |
