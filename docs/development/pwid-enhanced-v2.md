# AntX 增强权限模型设计 (PWID-Enhanced)

> **版本**: v2.0 | **状态**: ⚠️ **已被 v4 取代**
>
> 本文档描述的是 PWID v2 增强设计。当前代码实现的是 **v4 能力流动模型**，
> 详见 [pwid-model.md](pwid-model.md)。本文档保留作设计演进参考。
>
> v2→v4 的核心变更：废除了 `PWID_LEVEL_ROOT` 硬编码特权检查，改为每个
> PWID 自带 16×64 位能力掩码；原 Root 概念被 First Token 替代。

## 一、设计哲学

### 1.1 与传统模型的对比

| 维度 | Unix/Linux | Windows | **AntX PWID-E** |
|------|-----------|---------|----------------|
| 身份基础 | 用户名+密码 | SID | **密码→PWID (无用户)** |
| 权限粒度 | rwx/9位 | ACL/细粒度 | **能力域+信任链** |
| 授权方式 | 静态配置 | 静态+动态 | **动态上下文感知** |
| 提权机制 | sudo/setuid | UAC/Admin | **临时令牌(Token)** |
| 审计模型 | 事后审计 | 事件日志 | **实时追踪+证明** |

### 1.2 核心创新点

```
┌─────────────────────────────────────────────────────────────┐
│                  AntX 权限三角                              │
│                                                             │
│         ┌─────────────┐                                      │
│         │   能力域     │ ← 细粒度能力控制                   │
│         │ Capability   │                                    │
│         │    Domain    │                                    │
│         └──────┬───────┘                                    │
│                │                                            │
│    ┌───────────┼───────────┐                               │
│    ▼           ▼           ▼                               │
│ ┌────────┐ ┌────────┐ ┌────────┐                          │
│ │ 信任链  │ │ 上下文  │ │ 令牌   │                          │
│ │ Trust  │ │ Context│ │ Token  │                          │
│ │ Chain  │ │ Aware  │ │ System │                          │
│ └────────┘ └────────┘ └────────┘                          │
└─────────────────────────────────────────────────────────────┘
```

## 二、核心概念定义

### 2.1 能力域 (Capability Domain)

**创新点**: 摒弃传统的"读/写/执行"三位一体，改为**领域化能力**

```rust
/// 能力域标识 (16位，支持65536个域)
pub type CapDomain = u16;

/// 预定义能力域
pub const CAP_DOMAIN_SYSTEM: CapDomain = 0x0000;   // 系统内核
pub const CAP_DOMAIN_FS: CapDomain = 0x0001;       // 文件系统
pub const CAP_DOMAIN_NET: CapDomain = 0x0002;      // 网络
pub const CAP_DOMAIN_PROC: CapDomain = 0x0003;     // 进程管理
pub const CAP_DOMAIN_DEVICE: CapDomain = 0x0004;   // 设备访问
pub const CAP_DOMAIN_USER_MGMT: CapDomain = 0x0005; // 用户管理
pub const CAP_DOMAIN_CUSTOM_START: CapDomain = 0x0100; // 自定义域起始

/// 域内能力位 (64位，每个域支持64种操作)
pub type CapBits = u64;

// 文件系统域内的预定义能力
pub const FS_CAP_READ: CapBits = 1 << 0;
pub const FS_CAP_WRITE: CapBits = 1 << 1;
pub const FS_CAP_EXECUTE: CapBits = 1 << 2;
pub const FS_CAP_CREATE: CapBits = 1 << 3;
pub const FS_CAP_DELETE: CapBits = 1 << 4;
pub const FS_CAP_CHMOD: CapBits = 1 << 5;
pub const FS_CAP_CHOWN: CapBits = 1 << 6;
pub const FS_CAP_MOUNT: CapBits = 1 << 7;
pub const FS_CAP_LINK: CapBits = 1 << 8;

// 进程管理域内的预定义能力
pub const PROC_CAP_FORK: CapBits = 1 << 0;
pub const PROC_CAP_EXEC: CapBits = 1 << 1;
pub const PROC_CAP_KILL: CapBits = 1 << 2;
pub const PROC_CAP_DEBUG: CapBits = 1 << 3;
pub const PROC_CAP_NICE: CapBits = 1 << 4;
pub const PROC_CAP_SCHED: CapBits = 1 << 5;
```

### 2.2 信任链 (Trust Chain)

**创新点**: 用"信任关系"替代"组"，形成动态授权网络

```
传统Unix:
  用户 → 组 → 权限 (静态层级)

AntX Trust Chain:
  PWID_A ──[信任]──▶ PWID_B ──[信任]──▶ PWID_C
                    │                      │
                [委托]能力              [继承]能力
                    ▼                      ▼
              可操作资源A            可操作资源B
```

**信任级别定义**:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrustLevel {
    None = 0,        // 无信任
    Basic = 1,       // 基础信任：可查看元信息
    Operate = 2,     // 操作信任：可执行已授权操作
    Delegate = 3,    // 委托信任：可转授自己的部分权限
    Full = 4,        // 完全信任：等同于自己
}
```

**信任记录结构**:

```rust
#[derive(Debug, Clone)]
#[repr(C)]
pub struct TrustEntry {
    pub subject_pwid: u64,      // 被信任者PWID
    pub object_pwid: u64,       // 授信者PWID
    pub trust_level: TrustLevel,
    pub cap_domain: CapDomain,  // 适用域
    pub cap_mask: CapBits,     // 能力掩码（可授予的能力子集）
    pub expires_at: u64,        // 过期时间 (0=永不过期)
    pub conditions: u32,        // 条件标志
    pub created_time: u64,
}

// 条件标志
pub const TRUST_COND_TIME_LIMITED: u32 = 0x01;   // 有时间限制
pub const TRUST_COND_IP_RESTRICTED: u32 = 0x02;   // IP限制
pub const TRUST_COND_SINGLE_USE: u32 = 0x04;     // 单次使用
pub const TRUST_COND_REQUIRES_2FA: u32 = 0x08;   // 需要二次验证
```

### 2.3 上下文感知权限 (Context-Aware Permission)

**创新点**: 权限随环境变化，同一PWID在不同场景拥有不同权限

```rust
/// 权限上下文
#[derive(Debug, Clone)]
pub struct PermissionContext {
    /// 时间上下文
    pub time_context: TimeContext,
    
    /// 位置/路径上下文
    pub location_context: LocationContext,
    
    /// 会话上下文
    pub session_context: SessionContext,
    
    /// 设备上下文
    pub device_context: DeviceContext,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeContext {
    pub time_of_day: TimeOfDay,  // 工作时间/休息时间等
    pub day_of_week: u8,          // 工作日/周末
    pub is_holiday: bool,         // 节假日
}

/// 时间段定义
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum TimeOfDay {
    Any = 0,
    WorkHours = 1,        // 09:00-18:00
    OffHours = 2,         // 18:00-09:00
    Maintenance = 3,      // 维护时间窗口
    Emergency = 4,         // 紧急状态
}

#[derive(Debug, Clone)]
pub struct LocationContext {
    pub current_path: [u8; 256],  // 当前路径
    pub mount_point: [u8; 128],   // 所在挂载点
    pub depth_from_root: u8,      // 距离根目录深度
}

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub session_type: SessionType,
    pub login_method: LoginMethod,
    pub consecutive_failures: u8,  // 连续失败次数
    pub risk_score: u8,            // 风险评分 (0-100)
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum SessionType {
    Local = 0,
    SSH = 1,
    Serial = 2,
    GUI = 3,
    API = 4,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum LoginMethod {
    Password = 0,
    Token = 1,
    Key = 2,
    Biometric = 3,
    Elevated = 4,  // 临时提权
}
```

### 2.4 令牌系统 (Token System)

**创新点**: 替代sudo，实现更安全的临时提权

```rust
/// 令牌类型
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum TokenType {
    Elevation = 0,    // 提权令牌 (类似sudo)
    Delegation = 1,   // 委托令牌
    Session = 2,      // 会话令牌
    OneTime = 3,      // 一次性令牌
    Scoped = 4,       // 作用域限定令牌
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct PwidToken {
    pub token_id: u64,             // 唯一令牌ID
    pub issuer_pwid: u64,          // 签发者PWID
    pub holder_pwid: u64,          // 持有者PWID
    pub token_type: TokenType,
    pub cap_domains: [CapDomain; 8], // 允许的能力域
    pub capabilities: [CapBits; 8],  // 对应域的能力掩码
    pub scope_path: [u8; 256],     // 作用路径 (Scoped Token)
    pub valid_from: u64,
    pub valid_until: u64,
    pub max_uses: u32,              // 最大使用次数 (0=无限)
    pub current_uses: u32,
    pub flags: u32,
    pub signature: [u8; 64],        // 签名 (HMAC-SHA256)
}

// 令牌标志
pub const TOKEN_FLAG_SINGLE_COMMAND: u32 = 0x01;  // 仅单条命令有效
pub const TOKEN_FLAG_NO_TTY: u32 = 0x02;         // 禁止TTY交互
pub const TOKEN_FLAG_REQUIRE_CONFIRM: u32 = 0x04; // 需要确认提示
pub const TOKEN_FLAG_AUDIT_ALL: u32 = 0x08;      // 审计所有操作
pub const TOKEN_FLAG_EXPIRE_ON_IDLE: u32 = 0x10; // 空闲时自动过期
```

## 三、增强型PWID结构

### 3.1 扩展的pwid_entry

```rust
#[derive(Debug, Clone)]
#[repr(C)]
pub struct PwidEntryV2 {
    // === 原有字段 ===
    pub pwid: u64,
    pub level: u8,
    pub note: [i8; 32],
    pub password_hash: [u8; 32],
    pub flags: u8,
    pub created_time: u64,
    pub expires_at: u64,
    
    // === 新增字段 ===
    
    /// 能力矩阵 (每个域的能力位图)
    pub capability_matrix: [CapBits; 16],
    
    /// 默认信任级别 (对新创建资源的默认行为)
    pub default_trust_level: TrustLevel,
    
    /// 最大委托深度 (防止无限委托链)
    pub max_delegation_depth: u8,
    
    /// 上下文策略索引
    pub context_policy_id: u16,
    
    /// 审计级别
    pub audit_level: AuditLevel,
    
    /// 速率限制参数
    pub rate_limit: RateLimit,
    
    /// 安全评分 (0-1000)
    pub security_score: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum AuditLevel {
    None = 0,        // 无审计
    Critical = 1,    // 仅关键操作
    Important = 2,   // 重要操作
    All = 3,         // 所有操作
    Full = 4,        // 全量审计含数据
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RateLimit {
    pub max_ops_per_second: u16,
    pub max_auth_failures: u8,
    pub lockout_duration_secs: u32,
    pub cooldown_duration_secs: u32,
}
```

### 3.2 扩展的文件权限

```rust
/// 增强型文件权限 (替代简单的rwx)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VfsPermissionV2 {
    /// 传统rwx权限 (兼容性保留)
    pub traditional_perm: u16,
    
    /// 能力域权限表
    pub domain_permissions: [DomainPermission; 8],
    
    /// 特殊PWID白名单 (总是允许这些PWID)
    pub allowed_pwids: [u64; 4],
    pub allowed_count: u8,
    
    /// 特殊PWID黑名单 (总是拒绝这些PWID)
    pub denied_pwids: [u64; 4],
    pub denied_count: u8,
    
    /// 继承标志
    pub inherit_flags: InheritFlags,
    
    /// 强制性要求 (即使有权限也需满足)
    pub mandatory_requirements: MandatoryReqs,
    
    /// 上下文约束
    pub context_constraints: ContextConstraints,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DomainPermission {
    pub domain: CapDomain,
    pub cap_bits: CapBits,
    pub is_set: bool,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InheritFlags: u8 {
        const INHERIT_PERMS = 0x01;
        const INHERIT_TRUST_CHAIN = 0x02;
        const INHERIT_CONTEXT_POLICY = 0x04;
        const INHERIT_ACL = 0x08;
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MandatoryReqs {
    pub require_trust_level: Option<TrustLevel>,
    pub require_min_security_score: Option<u16>,
    pub require_session_type: Option<SessionType>,
    pub require_login_method: Option<LoginMethod>,
    pub require_time_of_day: Option<TimeOfDay>,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ContextConstraints {
    pub allowed_times: u8,  // TimeOfDay位掩码
    pub max_risk_score: u8,
    pub require_2fa_for_write: bool,
    pub idle_timeout_secs: u32,
}
```

## 四、权限检查算法

### 4.1 增强型权限检查流程

```
请求访问(path, operation, pwid, context)
            │
            ▼
    ┌───────────────────┐
    │  1. 基础检查      │
    │  - PWID有效性     │
    │  - 是否禁用       │
    │  - 是否过期       │
    └────────┬──────────┘
             │ 通过
             ▼
    ┌───────────────────┐
    │  2. 上下文检查     │
    │  - 时间约束       │
    │  - 位置约束       │
    │  - 会话状态       │
    │  - 风险评估       │
    └────────┬──────────┘
             │ 通过
             ▼
    ┌───────────────────┐
    │  3. 能力域检查     │
    │  - 域内能力位     │
    │  - 能力矩阵匹配   │
    └────────┬──────────┘
             │ 通过
             ▼
    ┌───────────────────┐
    │  4. 信任链检查     │
    │  - 直接所有者     │
    │  - 信任委托       │
    │  - 令牌验证       │
    └────────┬──────────┘
             │ 通过
             ▼
    ┌───────────────────┐
    │  5. 强制性要求     │
    │  - 最低安全分     │
    │  - 2FA要求        │
    └────────┬──────────┘
             │ 通过
             ▼
         ✅ 允许访问
```

### 4.2 Rust实现示例

```rust
impl HvFsData {
    /// 增强型权限检查
    pub fn check_permission_v2(
        &self,
        inode: &HvfsInode,
        pwid: u64,
        access_type: CapBits,
        domain: CapDomain,
        context: &PermissionContext,
    ) -> PermissionResult {
        
        // 1. 基础检查
        let entry = match self.get_pwid_entry(pwid) {
            Some(e) => e,
            None => return PermissionResult::Denied(DenyReason::InvalidPwid),
        };
        
        if entry.flags & PWID_FLAG_DISABLED != 0 {
            return PermissionResult::Denied(DenyReason::Disabled);
        }
        
        if entry.expires_at > 0 && Self::get_time() > entry.expires_at {
            return PermissionResult::Denied(DenyReason::Expired);
        }
        
        // 2. Root特权检查
        if entry.level == PWID_LEVEL_ROOT {
            return PermissionResult::Allowed { 
                source: AllowSource::RootPrivilege,
                audit_required: true 
            };
        }
        
        // 3. 上下文约束检查
        if let Some(denied) = self.check_context_constraints(inode, context) {
            return PermissionResult::Denied(denied);
        }
        
        // 4. 能力域检查
        let domain_idx = domain as usize % 16;
        let has_capability = (entry.capability_matrix[domain_idx] & access_type) == access_type;
        
        if !has_capability {
            return PermissionResult::Denied(DenyReason::InsufficientCapability);
        }
        
        // 5. 所有者检查
        if inode.owner_pwid == pwid {
            return PermissionResult::Allowed {
                source: AllowSource::Owner,
                audit_required: false,
            };
        }
        
        // 6. 信任链检查
        if let Some(trust) = self.check_trust_chain(pwid, inode.owner_pwid, domain) {
            let effective_caps = trust.cap_mask & access_type;
            if effective_caps == access_type {
                return PermissionResult::Allowed {
                    source: AllowSource::TrustChain(trust.subject_pwid),
                    audit_required: true,
                };
            }
        }
        
        // 7. 令牌检查
        if let Some(token) = self.find_valid_token(pwid, domain, context) {
            let token_caps = token.capabilities[domain as usize % 8] & access_type;
            if token_caps == access_type {
                return PermissionResult::Allowed {
                    source: AllowSource::Token(token.token_id),
                    audit_required: true,
                };
            }
        }
        
        // 8. 其他权限检查 (传统模式兼容)
        let other_perm = inode.pwid_perm & 0x07;
        if (other_perm as CapBits & access_type) == access_type {
            return PermissionResult::Allowed {
                source: AllowSource::OtherPermissions,
                audit_required: false,
            };
        }
        
        PermissionResult::Denied(DenyReason::NoPermission)
    }
    
    fn check_context_constraints(
        &self,
        inode: &HvfsInode,
        context: &PermissionContext,
    ) -> Option<DenyReason> {
        // 时间约束
        if let Some(allowed_times) = inode.context_constraints.allowed_times {
            if allowed_times & (1 << context.time_context.time_of_day as u8) == 0 {
                return Some(DenyReason::TimeConstraint);
            }
        }
        
        // 风险评分
        if context.session_context.risk_score > inode.context_constraints.max_risk_score {
            return Some(DenyReason::HighRisk);
        }
        
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResult {
    Allowed { source: AllowSource, audit_required: bool },
    Denied(DenyReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AllowSource {
    RootPrivilege,
    Owner,
    TrustChain(u64),  // 来源PWID
    Token(u64),       // 令牌ID
    OtherPermissions,
    ContextPolicy,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DenyReason {
    InvalidPwid,
    Disabled,
    Expired,
    InsufficientCapability,
    NoPermission,
    TimeConstraint,
    HighRisk,
    PathRestriction,
    TokenExpired,
    TokenExhausted,
    NotInScope,
    Missing2FA,
    RateLimited,
}
```

## 五、API接口设计

### 5.1 能力管理API

```c
// 能力查询
int pwid_has_capability(uint64_t pwid, CapDomain domain, CapBits caps);

// 能力授予 (仅Root或受托者可调用)
int pwid_grant_capability(uint64_t granter, uint64_t grantee, 
                         CapDomain domain, CapBits caps,
                         uint8_t trust_level, uint64_t duration_secs);

// 能力撤销
int pwid_revoke_capability(uint64_t revoker, uint64_t target,
                          CapDomain domain, CapBits caps);

// 能力委托
int pwid_delegate_capabilities(uint64_t delegator, uint64_t delegatee,
                              CapDomain domain, CapBits caps_to_delegate,
                              uint8_t max_depth, uint64_t expires_at);
```

### 5.2 信任链API

```c
// 建立信任关系
int pwid_add_trust(uint64_t truster, uint64_t trusted,
                   uint8_t level, CapDomain domain, CapBits mask,
                   uint64_t expires_at, uint32_t conditions);

// 移除信任关系
int pwid_remove_trust(uint64_t truster, uint64_t trusted);

// 查询信任链
int pwid_check_trust(uint64_t subject, uint64_t object, 
                     CapDomain domain, CapBits required_caps);

// 列出信任关系
void pwid_list_trusts(uint64_t pwid);
```

### 5.3 令牌API

```c
// 创建提权令牌 (类似sudo)
uint64_t pwid_create_elevation_token(
    uint64_t requester,
    const char *password,  // 验证密码
    CapDomain domains[],   // 允许的域
    CapBits capabilities[], // 各域能力
    uint32_t max_uses,
    uint32_t duration_secs,
    uint32_t flags
);

// 创建作用域令牌
uint64_t pwid_create_scoped_token(
    uint64_t issuer,
    uint64_t holder,
    const char *scope_path,  // 限制在此路径下
    CapDomain domain,
    CapBits caps,
    uint64_t expires_at
);

// 使用令牌
int pwid_use_token(uint64_t token_id, const char *command);

// 销毁令牌
int pwid_revoke_token(uint64_t token_id, uint64_t revoker);

// 列出活跃令牌
void pwid_list_tokens(uint64_t pwid);
```

### 5.4 上下文策略API

```c
// 创建上下文策略
uint16_t pwid_create_context_policy(
    uint64_t owner,
    TimeOfDay allowed_times,
    uint8_t max_risk_score,
    bool require_2fa_for_write,
    uint32_t idle_timeout
);

// 应用策略到PWID
int pwid_apply_context_policy(uint64_t pwid, uint16_t policy_id);

// 设置文件上下文约束
int vfs_set_context_constraints(const char *path, 
                                ContextConstraints *constraints,
                                uint64_t pwid);
```

## 六、使用示例

### 6.1 场景1：开发者权限配置

```bash
# 开发者PWID拥有完整FS和网络能力，但无内核操作能力
pwid --set-capabilities 0xABCD... \
    --domain=fs:rwce \
    --domain=net:all \
    --domain=proc:fork,exec \
    --default-trust=operate

# 允许开发者将部分FS权限委托给测试人员
pwid --add-trust 0xABCD... 0x1234... \
    --level=delegate \
    --domain=fs:r,w \
    --condition=single-use,time-limited
```

### 6.2 场景2：临时提权安装软件

```bash
# 创建一次性提权令牌，仅允许在/tmp下写入
TOKEN=$(pwid --create-elevation-token \
    --password="root_password" \
    --scope="/tmp" \
    --domains=fs:w \
    --max-uses=1 \
    --require-confirm)

# 使用令牌执行安装
pwid --use-token $TOKEN "make install"
# 令牌自动失效
```

### 6.3 场景3：时间敏感的操作

```bash
# 配置备份任务只能在维护时间窗口执行
pwid --create-context-policy backup-policy \
    --allowed-times=maintenance \
    --require-2fa-write=true

# 应用到备份PWID
pwid --apply-policy 0xBACKUP... backup-policy
```

## 七、与现有系统的兼容性

### 7.1 向后兼容

- **传统rwx权限仍然有效**
- 未使用新特性的PWID按原有逻辑工作
- Level 0 (Root) 行为不变

### 7.2 迁移路径

```
阶段1 (当前): 基础PWID + 三级权限
    ↓
阶段2 (本次): 添加能力域 + 信任链
    ↓  
阶段3 (未来): 完整上下文感知 + 令牌系统 + 审计
```

### 7.3 性能考虑

- **能力域查找**: O(1) 数组索引
- **信任链遍历**: 限制最大深度为8，O(8) ≈ O(1)
- **上下文计算**: 缓存最近结果，TTL=5秒
- **令牌验证**: 内存哈希表查找，O(1)均摊

## 八、安全分析

### 8.1 攻击面分析

| 攻击向量 | 传统防御 | 新增防御 |
|----------|----------|----------|
| 权限提升 | sudo审计 | 令牌有限生命周期+作用域限制 |
| 横向移动 | 组隔离 | 信任链深度限制+风险评分 |
| 时间窗口攻击 | 无 | 上下文时间约束+空闲超时 |
| 社会工程 | 密码强度 | 多因素+风险自适应 |

### 8.2 最小权限原则实现

```
传统: User → Group → Permissions (静态)
AntX:  PWID → Context → CapDomain → Caps → Resource (动态多层过滤)
```

每层都可独立控制，最终权限是各层的**交集**。

## 九、总结

### 9.1 核心优势

1. **保持AntX特色**: 无用户概念，密码即身份
2. **灵活度大幅提升**: 从3级→65536域×64能力的组合
3. **动态适应**: 上下文感知，权限随环境变化
4. **可追溯**: 信任链+令牌+审计，全程可追踪
5. **向后兼容**: 不破坏现有功能

### 9.2 实现优先级

| 优先级 | 功能 | 复杂度 |
|--------|------|--------|
| P0 | 能力域基础框架 | 中 |
| P0 | 增强型权限检查算法 | 中 |
| P1 | 信任链基础实现 | 高 |
| P1 | 令牌系统(提权) | 高 |
| P2 | 上下文感知 | 高 |
| P2 | 审计与日志 | 中 |
| P3 | GUI管理工具 | 低 |
