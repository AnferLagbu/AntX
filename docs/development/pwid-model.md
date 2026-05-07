# QueenX PWID 权限模型 v4：能力流动模型

> **最后更新**: 2026-05-08 | **状态**: ✅ 已实施
>
> **v3→v4 核心变更**: 废除固定等级权限映射，采用 PWID 自带能力掩码；原 Root 概念替换为创世令牌 + 全能力身份；`--first` 引导参数提供系统恢复通道。

## 一、设计原点

v3 模型声称"没有用户"，但原 Root（不可删除、天生全权限、备注硬编码）本质上就是一个超级用户。它只是把 `UID=0` 换成了 `level=0`，藏得深了一点。

v4 的核心思想：**没有任何身份天生有权。所有能力都是被授予的，且可以被收回。**

```
v3:  等级 → 固定权限           v4:  能力掩码 → 精确权限
     Root   → ALL                    caps=ALL → 全能力（可通过First Token创）
     Trusted→ FS_RW+Create           caps={FS_RW, NET_PING}
     Untrustworthy→FS_ReadOnly       caps={FS_READ}
```

## 二、核心概念

### 2.1 能力 (Capability)

每个 PWID 携带一个 64 位能力位掩码，不再从等级推导。等级降级为标签（不参与权限判断）：

| 组件 | v3 | v4 |
|------|-----|-----|
| 权限来源 | 等级 (level: 0/1/2/3) | 能力掩码 (capability_mask: u64×16) |
| 最高特权 | `level=0` → ALL | `caps=ALL`（无预设来源，必须被授予） |
| 无主状态 | 不存在（原 Root 永远存在） | 存在——所有全能力身份可被删除 |
| 恢复机制 | 无（密码丢失=永久锁定） | `--first` 引导参数 → First Token |

### 2.2 First Token（创世令牌）

系统首次启动时，无任何身份。内核检测到 PWID 表为空，生成 First Token：

```
System Boot → PWID table empty? → First Token (id=0, caps=ALL, max_uses=1)
                                        │
                                        ▼
                                  安装向导获取
                                        │
                                        ▼
                             创建第一个全能力 PWID
                                        │
                              Token 自动销毁
```

**特性**：
- 引导参数触发：`kernel --first` 或检测到 PWID 表为空时自动生成
- 一次性：`max_uses=1`，创建第一个身份后立即无效
- 无时间限制：`valid_until=0`（永不自然过期）
- Token ID 固定为 0
- 完全复用现有 Token 系统（`token.rs` / `PwidToken`），无需新数据结构

### 2.3 无主状态

如果最后一个全能力 PWID 被删除：

- 现有会话继续运行（已有能力不丢失）
- 无法创建新身份（无人持有 `USER_MGMT_CAP_CREATE`）
- 物理访问者可通过 `--first` 重建
- 这是特性，不是 bug——承认物理访问意味着主权

## 三、能力领域定义

从现有能力定义出发，补全五个核心领域：

```
领域 0: SYSTEM     — 系统配置（HOSTNAME, CONFIG, BOOT, CLEANUP）
领域 1: FS         — 文件系统（READ, WRITE, EXEC, CREATE, DELETE, CHMOD, CHOWN, MOUNT）
领域 2: NET        — 网络（PING, BIND, CONNECT, DNS, HTTP）
领域 3: PROC       — 进程（FORK, EXEC, KILL, DEBUG）
领域 4: DEVICE     — 设备（DISK_ADMIN, DISK_FORMAT, DISK_PARTITION）
领域 5: USER_MGMT  — 身份管理（CREATE, DELETE, LIST, TOKEN_ISSUE, TRUST_ADD）
```

每个领域 64 位，16 个领域槽位。`CapabilityMatrix`（已在 `capability.rs` 中定义）天然承载此结构。

## 四、权限模型（5 层检查，保持完整）

```
check_permission(pwid, inode, operation)
│
├─ L0: Disabled / Expired 检查         ← 新增，替代旧 root bypass
│   if disabled || expired → DENIED
│
├─ L1: Sensitivity Label               ← 不变
│   pwid.clearance >= inode.sensitivity
│
├─ L2: ACE List (per-file override)    ← 不变
│   匹配则 allow/deny 决定
│
├─ L3: Capability Matrix               ← 核心变更
│   pwid.capability_mask.has(domain, operation) ?
│   (不再查表映射 level→caps，直接查 PWID 自身)
│
└─ L4: Trust Chain                     ← 不变
    pwid → trust_entry → owner_pwid ?
```

## 五、能力授予规则

### 5.1 创建新 PWID

创建者指定目标能力子集。规则：

```
(caps_of_creator & caps_of_target) == caps_of_target
```

即：只能授予自己持有的能力。创建行为本身消耗 `USER_MGMT_CAP_CREATE`。

### 5.2 令牌提权

向目标身份的持有者验证密码 → 创建有时限的能力副本。与 v3 完全相同。

### 5.3 信任链委托

与 v3 完全相同。每跳能力取交集（`required_caps & trust.cap_mask`），8 跳限界。

## 六、等级降级为标签

等级字段保留但不决定权限：

```c
// v3 (旧) — 等级直接决定能力
if (pwid_get_level(pwid) == PWID_LEVEL_ROOT)
    sys_disk_format();

// v4 (新) — 能力检查
if (pwid_has_cap(pwid, CAP_DOMAIN_DEVICE, DEVICE_CAP_DISK_ADMIN))
    sys_disk_format();
```

等级仍可用作 UI 标签（显示"管理员"/"普通"/"访客"），但不再是权限门控。

## 七、未登录状态

`pwid_get_current()` 返回 0 时：

- 所有操作返回 `PERMISSION_DENIED`
- 唯一例外：`pwid_login()` 本身
- 系统启动后无人登录时进入安装向导或登录界面

## 八、多用户多会话支持

v4 对多用户支持**优于** v3：

| 场景 | v3 | v4 |
|------|-----|-----|
| 管理账户 | Root (等级=0) | 全能力 PWID |
| 日常账户 | Trusted (等级=1, 固定4位权限) | 精确能力集 (如 FS_RW + NET_FULL) |
| 访客账户 | Untrustworthy (只读) | 最小能力集 (FS_READ) |
| 服务账户 | 不存在 | 非交互式 (NET_BIND + PROC_FORK) |

多会话并发不受影响——每个终端独立 `pwid_login()`，各自获得独立的会话上下文。

## 九、恢复通道

| 场景 | v3 | v4 |
|------|-----|-----|
| 忘记密码 | **死锁** — 原 Root 无法重置 | `--first` → First Token → 创建新身份 |
| 物理访问 | 无后门 | 诚实承认：物理访问 = 主权 |

`--first` 可多次使用。它不是"灾难恢复模式"——它是"我要重新开始"。

## 十、API 变化

### 新增

```c
int  pwid_has_capability(uint64_t pwid, uint16_t domain, uint64_t required);
void pwid_set_capability(uint64_t pwid, uint16_t domain, uint64_t caps);
uint64_t pwid_get_capability(uint64_t pwid, uint16_t domain);
```

### 保留但标记废弃

```c
int  pwid_is_root(uint64_t pwid);           // @deprecated → pwid_has_capability(pwid, DOMAIN, ALL)
int  pwid_check_permission(pwid, level);     // @deprecated → 领域能力检查
```

### 行为变更

```c
pwid_get_fs_capability(pwid);  // 不再查表，返回 pwid.caps.domains[CAP_DOMAIN_FS]
pwid_get_current();            // 不再返回 guest PWID(0x0020F45A8B978417)，返回 0
```

## 十一、PWID 结构变化

```diff
  struct PwidEntry {
      pwid: u64,
      level: u8,              // 保留为标签
+     capability_mask: [u64; 16],  // 每个领域 64 位
      note: [u8; 128],
      password_hash: [u8; 32],
      // ... 其余不变
  }
```

## 十二、迁移兼容性

- 现有 `PwidEntry` 增加 `capability_mask` 字段（17×8=136 字节增量）
- 所有 level=0 的 PWID 默认赋予 `capability_mask[i]=0xFFFFFFFFFFFFFFFF`
- `get_level()` 查询仍可用，不返回错误
- 旧 syscall 在过渡期可同时接受新旧权限查询

---

**设计者**: Anfer + AI Assistant (2026-05-07)
**基于**: permission-model-v3.md（未废弃——5 层检查架构保持完整）
**取代**: pwid-model.md（原 Root 架构的描述）
