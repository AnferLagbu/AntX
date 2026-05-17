# 安全子系统

> PWID权限模型与身份管理

---

## 🎯 概述

AntX采用基于能力的权限模型（Capability-based），摒弃传统的Unix权限模型。

**核心组件**:
- **PWID**: 特权工作负载标识
- **Capability**: 能力矩阵
- **Session**: 会话管理
- **Audit**: 审计日志

---

## 🔐 PWID (Privileged Workload ID)

### 结构

```rust
pub struct Pwid {
    pub identity: u64,     // 身份标识（60位熵）
    pub level: u8,         // 特权等级（0=最高）
    pub flags: u8,         // 标志位
    pub reserved: u16,     // 保留
}
```

### 特权等级

| 等级 | 说明 | 权限 |
|------|------|------|
| 0 | 最高特权 | 所有操作 |
| 1-127 | 系统服务 | 大部分操作 |
| 128-254 | 用户进程 | 受限操作 |
| 255 | 最低特权 | 最少权限 |

---

## 🎯 能力矩阵

### 结构

```rust
pub struct CapabilityMatrix {
    pub caps: [u64; 16],   // 1024个能力位
}
```

### 能力域

| 域 | 范围 | 说明 |
|----|------|------|
| 0 | 0-63 | 文件系统能力 |
| 1 | 64-127 | 进程管理能力 |
| 2 | 128-191 | 内存管理能力 |
| 3 | 192-255 | 设备操作能力 |

### 能力检查

```rust
pub fn pwid_has_capability(
    pwid: u64,
    domain: u16,
    required: u64,
) -> bool
```

---

## 👤 会话管理

```rust
pub struct Session {
    pub session_id: u64,          // 会话ID
    pub pwid: u64,                // 关联PWID
    pub login_time: u64,          // 登录时间
    pub elevate_stack: Vec<u64>,  // 提权栈
}
```

---

## 📝 审计日志

```rust
pub struct AuditRecord {
    pub timestamp: u64,           // 时间戳
    pub pwid: u64,                // 操作者PWID
    pub action: AuditAction,      // 操作类型
    pub resource: u64,            // 资源标识
    pub result: AuditResult,      // 操作结果
}
```

---

**最后更新**: 2026-05-18
