# QueenX 权限模型 v3 → v4：5层权限架构

> **最后更新**: 2026-05-07 | **状态**: v4 实施中
>
> 本文档定义 QueenX 的 5 层权限检查架构。**L0/L1/L2/L4 各层自 v3 起不变**；L3 (Capability Matrix) 在 v4 中从"等级→固定能力"改为"PWID 自带能力掩码"。详见 `pwid-model.md`。

## 1. 概述

本文档定义 AntX/QueenX 内核的权限模型，以 5 层架构取代传统 Unix rwx 权限位。
核心原则：**身份驱动权限，而非文件携带权限**。

### 1.1 设计目标

| 目标 | 说明 |
|------|------|
| 废除 rwx | 不再依赖 per-inode 的 owner/group/other 三元组 |
| 废除组 (group) | 用 ACE 例外列表 + 信任链委托替代组管理 |
| 身份即权限 | PWID 自带能力矩阵 (v4: capability_mask)，文件只需敏感标签 |
| 信息流控制 | sensitivity 字段提供 BLP 风格的强制访问控制 |
| 可审计 | 每次拒绝携带结构化的 DenyReason |

### 1.2 与 Unix rwx 的对比

| 维度 | Unix rwx | QueenX v4 |
|------|----------|-----------|
| 文件权限 | 9 bits (owner/group/other) | 1 byte sensitivity + 可选 ACE |
| 身份粒度 | UID + GID（两组） | PWID（单一 64 位） |
| 操作粒度 | r/w/x 三位 | capability_mask: [u64; 16] |
| 特权通道 | root (UID=0) 硬编码 | First Token 一次性授予 + 令牌提权 |
| 委托 | sudo（进程外） | 信任链（内核内，8 跳限界） |
| 组 | /etc/group | 无 — ACE + 令牌替代 |
| 审计 | 无 | DenyReason 枚举（14 种原因） |

---

## 2. 三层架构

```
┌────────────────────────────────────────────────────────┐
│              check_permission(pwid, inode, op)          │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Layer 1: Sensitivity Label（敏感标签）                 │
│  ─────────────────────────────────────                 │
│  inode.sensitivity: u8  (0 = 公开, 255 = 仅内核)        │
│                                                        │
│  Rule: pwid.clearance >= inode.sensitivity              │
│  ↓ Yes          ↓ No                                    │
│  Continue       Check Token → No Token → Denied         │
│                                                        │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Layer 2: ACE List（访问控制条目）※ Phase 3 实现        │
│  ─────────────────────────────────────                 │
│  inode.ace_list: [ (pwid_pattern, cap_bits, allow/deny) ]│
│                                                        │
│  Rule: 匹配的 ACE ?                                     │
│  ↓ Yes          ↓ No                                    │
│  ACE 决定       Continue                                │
│                                                        │
├────────────────────────────────────────────────────────┤
│                                                        │
│  Layer 3: Capability Matrix（能力矩阵）※ v4: PWID自带    │
│  ─────────────────────────────────────                 │
│  pwid.capability_mask[domain] & operation               │
│                                                        │
│  (v4: 不再从等级推导，每个 PWID 独立的能力掩码)           │
│                                                        │
│  Rule: (pwid.caps & op) == op ?                        │
│  ↓ Yes          ↓ No                                    │
│  ALLOWED        DENIED(InsufficientCapability)           │
│                                                        │
└────────────────────────────────────────────────────────┘
```

---

## 3. 数据结构

### 3.1 Inode 字段变化

```
RamFS inode（Phase 2）:
  + sensitivity: u8        // 新增：敏感标签
  + ace_count: u8           // 新增：ACE 条目数 (Phase 3)
  + ace_offset: u32         // 新增：ACE 列表数据区偏移 (Phase 3)
    perm: u16               // 保留：Phase 5 移除
    owner_pwid: u64         // 保留：文件归属

HvFS inode（Phase 2）:
  + sensitivity: u8         // 新增
  + ace_count: u8           // 新增 (Phase 3)
  + ace_offset: u32         // 新增 (Phase 3)
    pwid_perm: u16          // 保留：Phase 5 移除
    owner_pwid: u64         // 保留
```

### 3.2 能力位定义

```c
#define FS_CAP_READ    (1ULL << 0)   // 读文件内容
#define FS_CAP_WRITE   (1ULL << 1)   // 写文件内容
#define FS_CAP_EXECUTE (1ULL << 2)   // 执行文件
#define FS_CAP_CREATE  (1ULL << 3)   // 创建新文件/目录
#define FS_CAP_DELETE  (1ULL << 4)   // 删除文件/目录
#define FS_CAP_CHMOD   (1ULL << 5)   // 修改敏感标签/ACE
#define FS_CAP_CHOWN   (1ULL << 6)   // 修改所有者
#define FS_CAP_MOUNT   (1ULL << 7)   // 挂载操作
```

### 3.3 操作→能力位映射

| VFS 操作 | 被检查对象 | 所需能力位 |
|----------|-----------|-----------|
| open(path, O_RDONLY) | 目标 inode | `FS_CAP_READ` |
| open(path, O_WRONLY) | 目标 inode | `FS_CAP_WRITE` |
| open(path, O_CREAT) | 父目录 inode | `FS_CAP_CREATE` |
| read(fd) | 目标 inode | `FS_CAP_READ` |
| write(fd) | 目标 inode | `FS_CAP_WRITE` |
| mkdir(path) | 父目录 inode | `FS_CAP_CREATE` |
| unlink(path) | 父目录 inode | `FS_CAP_DELETE` |
| stat(path) | 目标 inode | `FS_CAP_READ` |

---

## 4. 实现路线图

| 阶段 | 内容 | 依赖 | 状态 |
|------|------|------|------|
| Phase 1 | 设计文档、数据定义 | 无 | ✅ 本文档 |
| Phase 2 | sensitivity 字段 + check_permission v3 | 无 | 🚧 实现中 |
| Phase 3 | ACE 列表实现 | Phase 2 | 📋 计划 |
| Phase 4 | 信任链 + 令牌接入文件系统 | Phase 2 | 📋 计划 |
| Phase 5 | 移除 perm/pwid_perm 字段，清理 rwx | Phase 3 | 📋 计划 |

---

## 5. PWID 级别→默认能力映射

| 级别 | 数值 | FS 默认能力 | 说明 |
|------|------|-----------|------|
| Root | 0 | 全部位 (0xFFFFFFFFFFFFFFFF) | 操作系统管理 |
| Trusted | 1 | READ\|WRITE\|EXECUTE\|CREATE | 普通用户 |
| Standard | 2 | READ\|EXECUTE | 受限用户 |
| Untrustworthy | 3 | READ | 访客/沙箱 |

未来 Phase 4 中，每个 PWID 可携带独立的能力矩阵覆盖此默认值。

---

## 6. DenyReason 枚举（审计用）

```rust
pub enum DenyReason {
    InvalidPwid = 1,        // PWID 不存在
    Disabled = 2,           // 账户已禁用
    Expired = 3,            // 账户已过期
    InsufficientCapability = 4,  // 能力不足
    NoPermission = 5,       // 无权限
    TimeConstraint = 6,     // 时间窗口限制
    HighRisk = 7,           // 高风险会话
    PathRestriction = 8,    // 路径限制
    TokenExpired = 9,       // 令牌过期
    TokenExhausted = 10,    // 令牌用尽
    NotInScope = 11,        // 不在令牌范围内
    Missing2FA = 12,        // 需要二次认证
    RateLimited = 13,       // 速率限制
    NotAuthenticated = 14,   // 未认证
    SensitivityViolation = 15,  // 敏感标签不匹配 (新增)
}
```
