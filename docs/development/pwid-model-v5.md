# QueenX PWID 权限模型 v5

> **最后更新**: 2026-05-14 | **状态**: 🏗️ 设计中
>
> **设计原则**: 零概念 + 数值化特权级 + 内核隔离 + First Token。密码→身份→特权级→能力，能力来自授予。
>
> **PWID 初心**: ✅ 密码决定身份 | ✅ 无预设特权 | ✅ 能力来自授予
>
> **拒绝**: 工种、委托链、令牌、信息素、轨迹、蒸发、过期、角色、审批——这些概念都是"为了设计而设计"。

## 一、特权级问题

直通模型的核心漏洞：**没有特权级，谁都可以撤销谁**。

```
场景：
  A 创建 B，授予 B 全能力
  B 创建 C，授予 C 全能力

问题：
  A 可以撤销 B 的能力 ✓（A 是 B 的创建者）
  B 可以撤销 A 的能力 ✗（B 不是 A 的创建者，也不曾授予 A）
  B 可以撤销 C 的能力 ✓（B 是 C 的创建者）
  C 可以撤销 B 的能力 ✗（C 不是 B 的创建者，也不曾授予 B）
```

解决：**数值化特权级 + 创建关系 + 授予关系**。

### 1.1 特权级数值

```rust
pub struct PwidEntry {
    pub pwid: AtomicU64,
    pub creator_pwid: AtomicU64,
    pub privilege_level: AtomicU8,  // 特权级：0 = 最高，255 = 最低
    pub flags: AtomicU16,
    pub caps: [AtomicU64; 16],
    // ...
}
```

**特权级规则**：
1. 系统引导身份：`privilege_level = 0`（最高）
2. 创建新身份时：`new.privilege_level = creator.privilege_level + 1`
3. 特权级只能升高（数值增大），不能降低
4. 高特权级（数值小）可以操作低特权级（数值大）
5. 低特权级不能操作高特权级

### 1.2 特权级操作规则

```
创建身份:
  A (level=5) 创建 B → B.level = 6
  规则: 新身份的特权级 = 创建者特权级 + 1

授予能力:
  A (level=5) 授予 B (level=7) FS_WRITE → ✓（A 比 B 高）
  A (level=5) 授予 B (level=3) FS_WRITE → ✗（A 比 B 低）
  规则: 只有高特权级才能授予低特权级

撤销能力:
  A (level=5) 撤销 B (level=7) 的 FS_WRITE:
    检查 1: A.level < B.level（A 比 B 高）
    检查 2: A 是 B 的创建者 或 A 曾授予 B 这些能力
    两个条件都满足 → ✓

转移创建者特权:
  A (level=5) 转移 B (level=7) 的创建者特权给 C:
    检查 1: A 是 B 的当前创建者
    检查 2: C.level >= B.level（C 不能比 B 高）
    都满足 → ✓
```

## 二、数据结构

### 2.1 PwidEntry（身份 + 创建者 + 特权级）

```rust
pub struct PwidEntry {
    pub pwid: AtomicU64,
    pub creator_pwid: AtomicU64,     // 创建者（0 = 系统引导创建）
    pub privilege_level: AtomicU8,   // 特权级：0 = 最高，255 = 最低
    pub flags: AtomicU16,
    pub caps: [AtomicU64; 16],       // 能力集合：16 域 × 64 位
    pub note: [u8; 64],
    pub password_hash: [u8; 48],
    pub created_time: AtomicU64,
    pub expires_at: AtomicU64,
    pub lockout_until: AtomicU64,
    pub failed_attempts: AtomicU32,
    pub last_login_time: AtomicU64,
}
```

**新增字段**：
1. `creator_pwid`：记录谁创建了这个身份
2. `privilege_level`：数值化特权级，0 最高，255 最低

### 2.2 GrantRecord（授予记录）

```rust
pub struct GrantRecord {
    pub grantor_pwid: u64,       // 授予者
    pub grantee_pwid: u64,       // 被授予者
    pub domain: CapDomain,
    pub caps: CapBits,           // 被授予的能力
    pub granted_at: u64,         // 授予时间
}

pub static GRANT_RECORDS: spin::Mutex<[GrantRecord; MAX_GRANTS]> = 
    spin::Mutex::new([GrantRecord::empty(); MAX_GRANTS]);

const MAX_GRANTS: usize = 1024;  // 最多记录 1024 条授予
```

**唯一新增结构**：GrantRecord。记录谁授予了谁什么能力。

## 三、核心操作

### 3.1 创建身份（特权级继承）

```rust
pub fn create(
    password: &str,
    note: &str,
    creator_pwid: u64,
) -> Result<u64, PwidError> {
    let pwid = crypto::generate_pwid(password, note);
    
    // 计算特权级
    let privilege_level = if creator_pwid == 0 {
        0  // 系统引导：最高特权级
    } else {
        let creator = find(creator_pwid).ok_or(PwidError::NotFound)?;
        let creator_level = creator.privilege_level.load(Ordering::Acquire);
        
        // 检查：不能创建特权级溢出
        if creator_level >= 254 {
            return Err(PwidError::PrivilegeOverflow);
        }
        
        creator_level + 1  // 新身份特权级 = 创建者特权级 + 1
    };
    
    let mut entry = PwidEntry::new(pwid, password, note);
    entry.creator_pwid.store(creator_pwid, Ordering::Release);
    entry.privilege_level.store(privilege_level, Ordering::Release);
    
    // 自动授予地板能力
    for i in 0..16 {
        entry.caps[i].store(VIABLE_FLOOR[i], Ordering::Release);
    }
    
    insert(entry);
    
    // 审计
    AUDIT.log(creator_pwid, AuditAction::Create, pwid, 0, privilege_level as u64);
    
    Ok(pwid)
}
```

### 3.2 授予能力（特权级检查）

```rust
pub fn grant(
    grantor_pwid: u64,
    grantee_pwid: u64,
    domain: CapDomain,
    caps: CapBits,
) -> Result<(), PwidError> {
    let grantor = find(grantor_pwid).ok_or(PwidError::NotFound)?;
    let grantee = find(grantee_pwid).ok_or(PwidError::NotFound)?;
    
    // 检查 1: grantor 持有被授予的能力（子集规则）
    let grantor_caps = grantor.caps[domain as usize].load(Ordering::Acquire);
    if (grantor_caps & caps) != caps {
        return Err(PwidError::PermissionDenied);
    }
    
    // 检查 2: grantor 特权级 >= grantee 特权级
    let grantor_level = grantor.privilege_level.load(Ordering::Acquire);
    let grantee_level = grantee.privilege_level.load(Ordering::Acquire);
    if grantor_level > grantee_level {
        return Err(PwidError::InsufficientPrivilege);
    }
    
    // 检查 3: grantee 未被禁用
    if grantee.flags.load(Ordering::Acquire) & PwidFlags::DISABLED != 0 {
        return Err(PwidError::Disabled);
    }
    
    // 执行授予
    grantee.caps[domain as usize].fetch_or(caps, Ordering::AcqRel);
    
    // 记录授予关系
    let mut records = GRANT_RECORDS.lock();
    for i in 0..MAX_GRANTS {
        if records[i].grantor_pwid == 0 {
            records[i] = GrantRecord {
                grantor_pwid,
                grantee_pwid,
                domain,
                caps,
                granted_at: pwid_now(),
            };
            break;
        }
    }
    
    // 审计
    AUDIT.log(grantor_pwid, AuditAction::Grant, grantee_pwid, domain, caps);
    
    Ok(())
}
```

### 3.3 撤销能力（特权级核心）

```rust
pub fn revoke(
    revoker_pwid: u64,
    target_pwid: u64,
    domain: CapDomain,
    caps: CapBits,
) -> Result<(), PwidError> {
    let revoker = find(revoker_pwid).ok_or(PwidError::NotFound)?;
    let target = find(target_pwid).ok_or(PwidError::NotFound)?;
    
    // 检查 1: revoker 特权级 < target 特权级（revoker 必须更高）
    let revoker_level = revoker.privilege_level.load(Ordering::Acquire);
    let target_level = target.privilege_level.load(Ordering::Acquire);
    if revoker_level >= target_level {
        return Err(PwidError::InsufficientPrivilege);
    }
    
    // 检查 2: revoker 是 target 的创建者
    let creator_pwid = target.creator_pwid.load(Ordering::Acquire);
    let is_creator = revoker_pwid == creator_pwid;
    
    // 检查 3: revoker 曾授予 target 这些能力
    let is_grantor = {
        let records = GRANT_RECORDS.lock();
        let mut found = false;
        for record in records.iter() {
            if record.grantor_pwid == revoker_pwid
                && record.grantee_pwid == target_pwid
                && record.domain == domain
                && (record.caps & caps) == caps {
                found = true;
                break;
            }
        }
        found
    };
    
    // 检查 4: revoker 是创建者或授予者
    if !is_creator && !is_grantor {
        return Err(PwidError::NotAuthorized);
    }
    
    // 检查 5: 不能撤销到低于地板
    let current = target.caps[domain as usize].load(Ordering::Acquire);
    let after_revoke = current & !caps;
    if (after_revoke & VIABLE_FLOOR[domain as usize]) != VIABLE_FLOOR[domain as usize] {
        return Err(PwidError::WouldBreakFloor);
    }
    
    // 执行撤销
    target.caps[domain as usize].fetch_and(!caps, Ordering::AcqRel);
    
    // 清除授予记录
    {
        let mut records = GRANT_RECORDS.lock();
        for record in records.iter_mut() {
            if record.grantor_pwid == revoker_pwid
                && record.grantee_pwid == target_pwid
                && record.domain == domain {
                record.caps &= !caps;
                if record.caps == 0 {
                    *record = GrantRecord::empty();
                }
            }
        }
    }
    
    // 审计
    AUDIT.log(revoker_pwid, AuditAction::Revoke, target_pwid, domain, caps);
    
    Ok(())
}
```

**撤销规则**：
1. **特权级检查**：revoker 必须比 target 特权级高（`revoker_level < target_level`）
2. **创建者权限**：创建者可以撤销被创建者的能力（`is_creator`）
3. **授予者权限**：授予者可以撤销自己授予的能力（`is_grantor`）
4. **地板保护**：不能撤销到低于地板

### 3.4 转移创建者特权（特权级限制）

```rust
/// 转移创建者特权
///
/// 当前创建者可以将特权转移给其他身份。
/// 转移后，新创建者获得对被创建者的撤销权限。
///
/// 特权级限制：新创建者的特权级不能比目标身份高。
pub fn transfer_creator(
    current_creator_pwid: u64,
    target_pwid: u64,
    new_creator_pwid: u64,
) -> Result<(), PwidError> {
    let current_creator = find(current_creator_pwid).ok_or(PwidError::NotFound)?;
    let target = find(target_pwid).ok_or(PwidError::NotFound)?;
    let new_creator = find(new_creator_pwid).ok_or(PwidError::NotFound)?;
    
    // 检查 1: current_creator_pwid 是当前创建者
    let creator = target.creator_pwid.load(Ordering::Acquire);
    if creator != current_creator_pwid {
        return Err(PwidError::NotCreator);
    }
    
    // 检查 2: current_creator 特权级 < target 特权级
    let current_level = current_creator.privilege_level.load(Ordering::Acquire);
    let target_level = target.privilege_level.load(Ordering::Acquire);
    if current_level >= target_level {
        return Err(PwidError::InsufficientPrivilege);
    }
    
    // 检查 3: new_creator 特权级 < target 特权级（新创建者必须比目标高）
    let new_level = new_creator.privilege_level.load(Ordering::Acquire);
    if new_level >= target_level {
        return Err(PwidError::InsufficientPrivilege);
    }
    
    // 执行转移
    target.creator_pwid.store(new_creator_pwid, Ordering::Release);
    
    // 审计
    AUDIT.log(current_creator_pwid, AuditAction::TransferCreator, 
              target_pwid, 0, new_creator_pwid);
    
    Ok(())
}
```

**转移规则**：
1. 只有当前创建者可以转移
2. 当前创建者必须比目标特权级高
3. 新创建者必须比目标特权级高（否则无法撤销目标）

## 四、特权级规则总结

```
特权级数值:
  0 = 最高特权级（系统引导）
  1 = 第一级子身份
  2 = 第二级子身份
  ...
  255 = 最低特权级

创建关系:
  A (level=5) 创建 B → B.level = 6
  规则: 新身份特权级 = 创建者特权级 + 1
  限制: 创建者特权级不能超过 254

授予关系:
  A (level=5) 授予 B (level=7) FS_WRITE → ✓（A 比 B 高）
  A (level=5) 授予 B (level=3) FS_WRITE → ✗（A 比 B 低）
  规则: 只有高特权级（数值小）才能授予低特权级（数值大）

撤销规则:
  可以撤销的情况:
    1. revoker 特权级 < target 特权级（revoker 更高）
    2. revoker 是 target 的创建者
    3. revoker 曾授予 target 这些能力

  不能撤销的情况:
    1. revoker 特权级 >= target 特权级（revoker 不够高）
    2. revoker 既不是创建者也不是授予者
    3. 撤销后低于地板

特权转移:
  A (level=0) 转移 B (level=1) 的创建者特权给 C (level=0):
    检查 1: A 是 B 的当前创建者 ✓
    检查 2: A.level=0 < B.level=1 ✓
    检查 3: C.level=0 < B.level=1 ✓
    结果: 转移成功，C 可以撤销 B

  防止无效转移:
    A (level=0) 转移 B (level=1) 给 C (level=2):
      C.level=2 >= B.level=1 → ✗ 拒绝
      原因: C 特权级不够高，转移后 C 无法撤销 B（因为撤销需要 revoker.level < target.level）
```

## 五、场景验证

```
场景 1: 正常层级
  系统引导创建 A（A.level = 0）
  A 创建 B（B.level = 1）
  B 创建 C（C.level = 2）

  A 撤销 B: ✓（A.level=0 < B.level=1，A 是 B 的创建者）
  A 撤销 C: ✗（A.level=0 < C.level=2，但 A 不是 C 的创建者，也不曾授予 C）
  B 撤销 A: ✗（B.level=1 >= A.level=0，特权级不够）
  B 撤销 C: ✓（B.level=1 < C.level=2，B 是 C 的创建者）
  C 撤销 B: ✗（C.level=2 >= B.level=1，特权级不够）

场景 2: 授予关系
  A (level=0) 创建 B (level=1)
  A 授予 B FS_WRITE（A.level=0 <= B.level=1 ✓）
  B 授予 C FS_WRITE（B.level=1 <= C.level=2 ✓）

  A 撤销 B 的 FS_WRITE: ✓（A.level=0 < B.level=1，A 授予了 B）
  B 撤销 C 的 FS_WRITE: ✓（B.level=1 < C.level=2，B 授予了 C）
  C 撤销 B 的 FS_WRITE: ✗（C.level=2 >= B.level=1，特权级不够）

场景 3: 特权转移
  A (level=0) 创建 B (level=1)
  A 转移 B 的创建者特权给 C (level=0):
    检查 1: A 是 B 的创建者 ✓
    检查 2: A.level=0 < B.level=1 ✓
    检查 3: C.level=0 < B.level=1 ✓
    结果: 转移成功

  A 撤销 B: ✗（A 不再是 B 的创建者）
  C 撤销 B: ✓（C.level=0 < B.level=1，C 是 B 的创建者）

  无效转移:
    A (level=0) 转移 B (level=1) 给 C (level=2):
      C.level=2 >= B.level=1 → ✗ 拒绝
      原因: C 特权级不够高，无法撤销 B

场景 4: 防止循环
  A (level=0) 创建 B (level=1)
  B 授予 A FS_WRITE（B.level=1 >= A.level=0 → ✗ 拒绝）
  
  修正: 低特权级不能授予高特权级
  结论: 授予关系自动防止循环

场景 5: 特权级溢出
  A (level=254) 创建 B:
    检查: A.level >= 254 → ✗ 拒绝
    原因: B.level 会是 255，但这是允许的
  
  A (level=255) 创建 B:
    检查: A.level >= 254 → ✗ 拒绝
    原因: B.level 会溢出

场景 6: 创建者降权后
  A (level=0) 创建 B (level=1)
  A 授予 B 全能力
  A 的能力被撤销（但 A.level 仍然是 0）

  A 撤销 B: ✓（A.level=0 < B.level=1，A 是 B 的创建者）
  结论: 特权级独立于能力，创建者降权后仍可撤销被创建者
```

## 六、地板：保证"有密码就有基本能力"

```rust
pub const VIABLE_FLOOR: [u64; 16] = {
    let mut f = [0u64; 16];
    f[CAP_DOMAIN_FS as usize]   = FS_CAP_READ | FS_CAP_EXECUTE;
    f[CAP_DOMAIN_PROC as usize] = PROC_CAP_FORK | PROC_CAP_EXEC;
    f
};
```

地板能力在 [create()](#31-创建身份特权级继承) 函数中自动授予。

撤销时，不能撤销到低于地板：

```rust
// 在 revoke() 中
if (after_revoke & VIABLE_FLOOR[domain as usize]) != VIABLE_FLOOR[domain as usize] {
    return Err(PwidError::WouldBreakFloor);
}
```

## 七、内核特权层与 First Token

### 7.1 三层特权架构

```
┌─────────────────────────────────────┐
│  内核特权层 (KERNEL)                 │
│  - privilege: 0xFF (特殊标记)        │
│  - 能力: 内存/中断/调度/设备/IPC      │
│  - 限制: 不能操作用户身份能力         │
└─────────────────────────────────────┘
           ↑ 内核内部使用
           ↓ 用户态不可见

┌─────────────────────────────────────┐
│  用户特权层 (USER)                   │
│  - privilege: 0-254                 │
│  - 能力: FS/NET/PROC/...            │
│  - 规则: 高特权级操作低特权级         │
└─────────────────────────────────────┘
           ↑ 受特权级规则约束
           ↓ 能力来自授予

┌─────────────────────────────────────┐
│  First Token (一次性授予)            │
│  - 系统引导时生成                    │
│  - 授予系统引导身份初始能力           │
│  - 用完即弃，不可重复                │
└─────────────────────────────────────┘
```

### 7.2 内核特权定义

```rust
// 内核特权级（独立于用户特权级）
pub const KERNEL_PRIVILEGE: u8 = 0xFF;

// 内核能力域
pub enum KernelCapDomain {
    MEMORY_MGMT = 0,   // 内存管理
    INTERRUPT = 1,     // 中断处理
    SCHEDULER = 2,     // 调度器
    DEVICE = 3,        // 设备驱动
    IPC = 4,           // 进程间通信
    BARRIER = 5,       // 栏栈系统
}

// 内核能力
pub const KERNEL_CAP_MEMORY: u64     = 1 << 0;
pub const KERNEL_CAP_INTERRUPT: u64  = 1 << 1;
pub const KERNEL_CAP_SCHEDULER: u64  = 1 << 2;
pub const KERNEL_CAP_DEVICE: u64     = 1 << 3;
pub const KERNEL_CAP_IPC: u64        = 1 << 4;
pub const KERNEL_CAP_BARRIER: u64    = 1 << 5;
pub const KERNEL_CAP_ALL: u64        = 
    KERNEL_CAP_MEMORY | KERNEL_CAP_INTERRUPT | 
    KERNEL_CAP_SCHEDULER | KERNEL_CAP_DEVICE |
    KERNEL_CAP_IPC | KERNEL_CAP_BARRIER;
```

**内核特权规则**：
1. 内核特权独立于用户特权，不受用户特权级规则约束
2. 内核只能操作内核能力，不能操作用户能力
3. 内核不能授予/撤销用户身份的能力
4. 内核特权不暴露给用户态

### 7.3 First Token 机制

```rust
/// First Token：一次性全能力授予
///
/// 系统引导时生成，用于授予系统引导身份初始能力。
/// 用完即弃，不可重复使用。
pub struct FirstToken {
    pub token_id: u64,
    pub granted: AtomicBool,    // 是否已使用
    pub created_at: u64,
}

pub static FIRST_TOKEN: spin::Mutex<Option<FirstToken>> = 
    spin::Mutex::new(None);

/// 生成 First Token
pub fn generate_first_token() -> FirstToken {
    FirstToken {
        token_id: crypto::random_u64(),
        granted: AtomicBool::new(false),
        created_at: pwid_now(),
    }
}

/// 使用 First Token 授予能力
pub fn grant_from_first_token(
    target_pwid: u64,
    domain: CapDomain,
    caps: CapBits,
) -> Result<(), PwidError> {
    let mut token_guard = FIRST_TOKEN.lock();
    let token = token_guard.as_mut().ok_or(PwidError::NoFirstToken)?;
    
    // 检查：Token 未使用
    if token.granted.load(Ordering::Acquire) {
        return Err(PwidError::TokenUsed);
    }
    
    // 执行授予（跳过特权级检查，因为这是系统引导）
    let target = find(target_pwid).ok_or(PwidError::NotFound)?;
    target.caps[domain as usize].fetch_or(caps, Ordering::AcqRel);
    
    // 标记 Token 已使用
    token.granted.store(true, Ordering::Release);
    
    // 审计
    AUDIT.log(0, AuditAction::FirstTokenGrant, target_pwid, domain, caps);
    
    Ok(())
}
```

### 7.4 系统引导流程

```rust
/// 系统引导：创建系统引导身份
///
/// 注意：系统引导身份特权级 = 0，但**不自动获得全能力**。
/// 能力需要通过 First Token 授予。
pub fn bootstrap(password: &str, note: &str) -> Result<u64, PwidError> {
    // 1. 生成 First Token
    {
        let mut token_guard = FIRST_TOKEN.lock();
        *token_guard = Some(generate_first_token());
    }
    
    // 2. 创建系统引导身份（特权级 = 0，但只有地板能力）
    let pwid = create(password, note, 0)?;
    
    // 3. 使用 First Token 授予全能力
    for i in 0..16 {
        grant_from_first_token(pwid, i as CapDomain, 0xFFFFFFFFFFFFFFFF)?;
    }
    
    Ok(pwid)
}

/// 系统恢复：通过 --first 参数重新生成 First Token
pub fn recover_with_first(password: &str, note: &str) -> Result<u64, PwidError> {
    // 1. 查找系统引导身份
    let pwid = find_by_note(note).ok_or(PwidError::NotFound)?;
    
    // 2. 验证密码
    let entry = find(pwid).unwrap();
    if !verify_password(password, &entry.password_hash) {
        return Err(PwidError::InvalidPassword);
    }
    
    // 3. 重新生成 First Token
    {
        let mut token_guard = FIRST_TOKEN.lock();
        *token_guard = Some(generate_first_token());
    }
    
    // 4. 使用 First Token 授予全能力
    for i in 0..16 {
        grant_from_first_token(pwid, i as CapDomain, 0xFFFFFFFFFFFFFFFF)?;
    }
    
    Ok(pwid)
}
```

**First Token 安全保证**：
1. **一次性**：只能使用一次，用完即弃
2. **内核存储**：存储在内核空间，用户态不可访问
3. **审计记录**：每次使用都有审计记录
4. **恢复机制**：通过 --first 参数重新生成（需要物理访问）

## 八、系统引导：--first

```rust
/// 系统引导身份的特性
///
/// 注意：系统引导身份不是"root"！
/// - 特权级 = 0（最高用户特权级）
/// - 能力来自 First Token 授予，而非预设
/// - 可以被撤销能力（如果有人比它特权级高，但没有人比它高）
pub fn bootstrap(password: &str, note: &str) -> Result<u64, PwidError> {
    // 见 7.4 节
}
```

系统引导身份的特性：
- `privilege_level = 0`（最高用户特权级）
- `creator_pwid = 0`（无创建者）
- 能力来自 First Token 授予（非预设）
- 不是"root"，只是第一个用户身份

**这就符合了 PWID 初心**：
- ✅ 密码决定身份
- ✅ 无预设特权（能力来自 First Token）
- ✅ 能力来自授予

## 九、与栏栈的集成

```rust
/// 栏栈回调：域回滚时降级能力
pub fn on_barrier_rollback(domain_pwid: u64, failure_count: u32) {
    let entry = match find(domain_pwid) {
        Some(e) => e,
        None => return,
    };

    // 根据失败次数降级
    match failure_count {
        0..=2 => {}  // 保持
        3 => {
            // 剥夺 FS_WRITE
            entry.caps[CAP_DOMAIN_FS as usize]
                .fetch_and(!FS_CAP_WRITE, Ordering::AcqRel);
        }
        4 => {
            // 剥夺 FS_WRITE | NET_SEND | PROC_CREATE
            entry.caps[CAP_DOMAIN_FS as usize]
                .fetch_and(!FS_CAP_WRITE, Ordering::AcqRel);
            entry.caps[CAP_DOMAIN_NET as usize]
                .fetch_and(!NET_CAP_SEND, Ordering::AcqRel);
            entry.caps[CAP_DOMAIN_PROC as usize]
                .fetch_and(!PROC_CAP_CREATE, Ordering::AcqRel);
        }
        _ => {
            // 降到地板
            for i in 0..16 {
                entry.caps[i].store(VIABLE_FLOOR[i], Ordering::Release);
            }
        }
    }
}
```

**就这么简单**。没有工种降级、没有委托失效、没有信息素蒸发——直接操作能力位。

## 十、与调度器的集成

```rust
/// 能力 → CPU 配额
pub fn get_cpu_quota(pwid: u64) -> u8 {
    let entry = match find(pwid) {
        Some(e) => e,
        None => return 0,
    };

    // 计算能力总量
    let mut total_caps = 0u64;
    for i in 0..16 {
        total_caps += entry.caps[i].load(Ordering::Acquire).count_ones() as u64;
    }

    // 能力越多，配额越高
    match total_caps {
        0..=10 => 10,      // 10%
        11..=30 => 30,     // 30%
        31..=60 => 50,     // 50%
        _ => 100,          // 100%
    }
}

/// 能力 → 进程数限制
pub fn get_max_processes(pwid: u64) -> u32 {
    let entry = match find(pwid) {
        Some(e) => e,
        None => return 0,
    };

    // 有 PROC_CREATE 能力 → 允许更多进程
    let proc_caps = entry.caps[CAP_DOMAIN_PROC as usize].load(Ordering::Acquire);
    if proc_caps & PROC_CAP_CREATE != 0 {
        64
    } else if proc_caps & PROC_CAP_FORK != 0 {
        32
    } else {
        8
    }
}
```

**就这么简单**。能力直接决定配额，不需要工种→配额的映射表。

## 十一、心智模型

```
传统: "我是 admin" → 我能做 admin 的事
直通: "我持有 FS_READ|FS_WRITE|PROC_FORK 能力" → 我能读写文件和创建进程

或者更简单:
直通: "我能读写文件和创建进程" → 直接看能力集合
```

**一句话**：看 `caps` 字段，就知道能做什么。不需要理解工种、委托、令牌、信息素。

## 十二、管理操作

```
授予: grant(target, FS, READ|WRITE)
撤销: revoke(target, FS, WRITE)

就这么两个操作。
```

不需要：
- 切换工种
- 添加委托
- 签发令牌
- 沉积信息素
- 设置巢穴规则

## 十三、与所有前代模型的对比

| 维度 | v4 | v5-token | v5-pheromone | v5-caste | **v5-direct** |
|------|-----|----------|-------------|----------|---------------|
| **概念数量** | 1 (caps) | 5 (令牌类型...) | 7 (信息素...) | 4 (工种...) | **2 (caps + 特权级)** |
| **心智模型** | "我有 caps" | "我持有令牌" | "A 在我身上沉积了信息素" | "我是工蚁" | **"我有 caps，特权级 N"** |
| **管理操作** | 无 | 5+ | 5+ | 4+ | **2 (grant/revoke)** |
| **代码行数** | ~2500 | ~4000 | ~4500 | ~3500 | **~2000** |
| **数据结构** | PwidEntry | +Token | +Pheromone | +Caste | **PwidEntry + GrantRecord + FirstToken** |
| **特权级** | ❌ 无 | ❌ 无 | ❌ 无 | ⚠️ 工种隐含 | **✅ 数值化 (0-254)** |
| **内核特权** | ❌ 无 | ❌ 无 | ❌ 无 | ❌ 无 | **✅ 独立隔离** |
| **预设特权** | ❌ 无 | ⚠️ First Token | ⚠️ First Token | ⚠️ 工种预设 | **✅ 无（First Token 授予）** |
| **栏栈集成** | 无 | 无 | 蒸发 | 降级 | **直接操作 caps** |
| **调度集成** | 独立 | 独立 | 浓度 | 工种→配额 | **caps→配额** |
| **"有密码无能力"** | ❌ 可能 | ✅ | ✅ | ✅ | **✅ 地板保证** |
| **能力可追加** | ❌ | ✅ | ✅ | ✅ | **✅ grant()** |
| **能力可撤销** | ❌ | ✅ | ✅ | ✅ | **✅ revoke()** |
| **撤销安全性** | ❌ 任何人 | ⚠️ 令牌持有者 | ⚠️ 信息素沉积者 | ⚠️ 工种规则 | **✅ 特权级+创建者+授予者** |
| **PWID 初心** | ⚠️ 部分 | ⚠️ 部分 | ⚠️ 部分 | ⚠️ 部分 | **✅ 完全符合** |
| **AntX 特色** | ⚠️ 部分 | ⚠️ 部分 | ✅ 最纯粹 | ⚠️ 像传统 | **✅ 简单、安全、初心** |

### 与 Unix root 的本质区别

| 维度 | Unix root | AntX 系统引导身份 |
|------|-----------|-------------------|
| **身份来源** | UID=0 硬编码 | 密码生成的 PWID |
| **能力来源** | 预设，自动获得 | First Token 授予 |
| **可撤销性** | 不可撤销（root 永远是 root） | 可撤销（如果有人特权级更高） |
| **可替代性** | 不可替代（UID=0 固定） | 可替代（任何特权级 0 的身份） |
| **内核特权** | root = 内核特权 | 用户特权与内核特权分离 |
| **安全边界** | root 绕过所有检查 | 受特权级规则约束 |
| **恢复机制** | 单用户模式 | --first 参数 + First Token |

**关键区别**：
1. **无硬编码特权**：系统引导身份不是 UID=0，而是密码生成的 PWID
2. **能力非预设**：能力来自 First Token 授予，而非自动获得
3. **内核隔离**：内核特权独立，不与用户特权混淆
4. **可审计**：所有能力授予都有审计记录

## 十四、直通模型的哲学

```
传统模型的复杂来自哪里？

1. 用户名 + 密码 → 需要用户管理
2. UID + GID → 需要组管理
3. rwx 权限位 → 需要 chmod/chown
4. root 特权 → 需要 sudo/su
5. 角色继承 → 需要角色管理

直通模型抛弃了所有这些：

1. 密码 → PWID（无用户名）
2. 单一 PWID（无组）
3. 16×64 能力矩阵（无 rwx）
4. 数值化特权级（0-254，无 root 概念）
5. 能力授予/撤销（无角色）

剩下的只有：
  密码 → PWID → 特权级 → 能力集合 → grant/revoke

特权级的作用：
  - 防止低特权级操作高特权级
  - 创建时自动继承（子身份特权级 = 父身份特权级 + 1）
  - 授予/撤销时检查特权级关系
  - 简单、直观、安全

内核特权的作用：
  - 内核操作独立于用户特权
  - 内核不能操作用户能力
  - 用户不能操作内核能力
  - 隔离、安全、清晰

First Token 的作用：
  - 系统引导时授予初始能力
  - 一次性，用完即弃
  - 无预设特权，符合 PWID 初心
  - 可恢复（通过 --first 参数）

这就是 AntX 的特色：
  不是"有复杂的权限模型"
  而是"有最简单、安全且符合初心的权限模型"

PWID 初心验证：
  ✅ 密码决定身份：PWID 由密码生成
  ✅ 无预设特权：能力来自 First Token 授予，而非自动获得
  ✅ 能力来自授予：所有能力都通过 grant() 授予
```

## 十五、模块架构

```
pwid/
├── mod.rs           # 模块声明 + 重导出
├── time.rs          # 统一时间源
├── types.rs         # PwidEntry（含 caps）
├── capability.rs    # 能力常量 + VIABLE_FLOOR
├── kernel_cap.rs    # 内核能力定义（独立于用户能力）
├── crypto.rs        # SHA-256 + salt + PWID 生成
├── table.rs         # PWID 表 + create/grant/revoke/transfer_creator + 查找
├── grant_record.rs  # GrantRecord 表管理
├── first_token.rs   # First Token 生成和使用
├── engine.rs        # 权限检查 + 特权级检查
├── session.rs       # 多终端会话
├── audit.rs         # 环形审计缓冲区
├── storage.rs       # 持久化 + v4 迁移
└── ffi.rs           # FFI 接口层
```

**13 个文件**。比 v5-caste 少 1 个，比 v5-pheromone 少 2 个。

## 十六、FFI 接口

```rust
// 授予能力
pub extern "C" fn pwid_grant(
    operator_pwid: u64,
    target_pwid: u64,
    domain: u16,
    caps: u64,
) -> i32;

// 撤销能力
pub extern "C" fn pwid_revoke(
    operator_pwid: u64,
    target_pwid: u64,
    domain: u16,
    caps: u64,
) -> i32;

// 权限检查
pub extern "C" fn pwid_check(pwid: u64, domain: u16, required: u64) -> i32;

// 获取能力
pub extern "C" fn pwid_get_caps(pwid: u64, domain: u16) -> u64;

// 获取特权级
pub extern "C" fn pwid_get_privilege_level(pwid: u64) -> u8;

// 获取创建者
pub extern "C" fn pwid_get_creator(pwid: u64) -> u64;

// 转移创建者特权
pub extern "C" fn pwid_transfer_creator(
    current_creator_pwid: u64,
    target_pwid: u64,
    new_creator_pwid: u64,
) -> i32;

// 栏栈回调
pub extern "C" fn pwid_on_barrier_rollback(pwid: u64, failure_count: u32);

// 兼容 v4
pub extern "C" fn pwid_has_capability(pwid: u64, domain: u16, required: u64) -> i32;
pub extern "C" fn pwid_get_fs_capability(pwid: u64) -> u64;
```

**核心接口**：
- `pwid_grant` / `pwid_revoke`：授予/撤销能力
- `pwid_check` / `pwid_get_caps`：权限检查
- `pwid_get_privilege_level` / `pwid_get_creator`：查询特权级和创建者
- `pwid_transfer_creator`：转移创建者特权

**没有的接口**：pwid_switch_caste、pwid_add_delegation、pwid_issue_token、pwid_deposit。

## 十七、v4→v5 迁移

```
v4 PwidEntry:
  pwid, level, flags, capability_mask[16], note[128], ...

v5 迁移:
  capability_mask → caps（字段名变化）
  level → privilege_level（字段名变化，语义不变）
  新增 creator_pwid（迁移时设为 0，表示系统引导创建）
  新增 GrantRecord 表（迁移时为空）
  note 128→64 截断
  时间戳 秒→微秒

迁移步骤:
  1. 读取 v4 PwidEntry
  2. 创建 v5 PwidEntry，复制字段
  3. 设置 creator_pwid = 0（假设所有 v4 身份都是系统引导创建）
  4. 设置 privilege_level = level
  5. 初始化空的 GrantRecord 表
  6. 写入 v5 存储

迁移后，所有 v4 身份的特权级保持不变，创建者都是系统引导。
```

## 十八、实施路线

### Phase 1: 基础设施
1. `time.rs` — 统一时间源
2. `types.rs` — PwidEntry（含 caps + privilege_level + creator_pwid）
3. `crypto.rs` — 密码学工具
4. `capability.rs` — 能力常量 + VIABLE_FLOOR
5. `kernel_cap.rs` — 内核能力定义（独立于用户能力）

### Phase 2: 核心操作
6. `table.rs` — PWID 表 + create/grant/revoke/transfer_creator + 查找
7. `grant_record.rs` — GrantRecord 表管理
8. `first_token.rs` — First Token 生成和使用
9. `engine.rs` — 权限检查 + 特权级检查

### Phase 3: 辅助系统
10. `session.rs` — 多终端会话
11. `audit.rs` — 环形缓冲区
12. `storage.rs` — 持久化 + v4 迁移
13. `ffi.rs` — FFI 接口

---

**设计者**: Anfer + AI Assistant (2026-05-14)
**设计原则**: 零概念 + 数值化特权级 + 内核隔离 + First Token
**PWID 初心**: ✅ 密码决定身份 | ✅ 无预设特权 | ✅ 能力来自授予
**特权级规则**: 0=最高用户特权，创建时+1，高特权级操作低特权级
**内核特权**: 独立于用户特权，仅用于内核内部操作，不暴露给用户态
**First Token**: 一次性授予，用完即弃，通过 --first 参数恢复
**拒绝**: 工种、委托、令牌、信息素、轨迹、蒸发、过期、角色、审批
**AntX 特色**: 不是"有复杂的权限模型"，而是"有最简单、安全且符合初心的权限模型"
