# PWID v4 安全审计报告 — 2026-05-09

> 审查范围: `src/pwid/` 全部 13 个源文件 + 调用链追踪（syscall.c / VFS / 调度器）
> 发现问题: 22 个代码缺陷 + 4 个系统可用性缺陷

---

## 深度排查：系统性可用障碍

以下是**追踪实际调用链**后发现的、会阻止系统正常运行的缺陷。编号接代码审计的 #22。

### 能力语义反置（最高优先级）

**#23. 权限模型的核心语义是反的 — "零能力 = 全通"**

这不是一个 bug——是**整个能力模型的基础语义错误**。追溯发现，三个关键路径都执行了这个错误语义：

| 调用路径 | 文件:行 | 逻辑 |
|---------|--------|------|
| VFS → `pwid_enhanced_check` | `ffi.rs:L749-L755` | `caps==0 → return 1 (Allowed)` |
| HvFS → `check_permission` | `hvfs.rs:L817-L820` | `caps==0 → return true` |
| RamFS → `check_permission` | `ramfs.rs` | 同上模式 |

这意味着：
- 新注册的身份默认无能力 → **但是无能力的身份拥有所有权限**
- 一旦给身份设置能力 → 反而**开始被限制**
- 攻击方式：直接传 `pwid=0` 或任何未注册的值 → 全通

**影响范围**：整个文件系统、用户管理、设备操作——所有经过这些检查的操作全部对未认证请求开放。

正确语义应该是：`caps==0 → Denied`，然后通过 First Token 创建的初始身份拥有全能力掩码。

### 调用链正确但结果为否的路径

**#24. syscall.c 的 12 个 `pwid_has_cap_raw` 检查全部会拒绝非全能力身份**

- `pwid_has_cap_raw` → `pwid_get_capability_raw` → FFI → Rust → 正常
- 但每个检查都以 `CAP_DOMAIN_SYS_ADMIN = 0xFFFFFFFFFFFFFFFF` 作为参数
- 这等价于旧的 `pwid_is_root()`——只有全能力掩码身份能通过
- 如果 First Identity 创建成功（`all_caps = [u64::MAX;16]`）→ 它可以通过
- 如果 First Identity 从未创建（无持久化）→ **全部 12 个管理员操作对所有人都不可用**

这是一个架构性问题——当前没有"部分 sys_admin"的概念，管理操作是二元的。

### 功能路径未实现的

**#25. First Token / Genesis 路径存在于代码中但触发路径模糊**

- `create_first_identity()` 确实创建全能力身份
- `pwid_any_identity_exists()` 存在
- 但 `main.c` 中只调了 `pwid_init()`，没有 `if !any_identity_exists() { request_genesis() }` 的逻辑
- 实际效果：内核启动后 PWID 表为空，必须用户态主动调用 `SYS_AUTH_CREATE_FIRST`——**但用户态程序自身就在 Ring 3，它如何引导第一个特权身份？**

这是一个自举问题（bootstrap problem）：系统没有身份 → 受限身份无法创建特权身份 → 永远无法启动。

**#26. 持久化不存在 → 每次重启都回到自举问题**

`storage.rs` 完全是 TODO。所有 PWID 在内存中，重启消失。当前测试能通过是因为测试框架在每次启动时重新注册 PWID——但那是测试代码，不是生产代码。

---

## 补充代码审计发现

### #27. `manager.create_internal` 仍然调用 `PwidLevel::Root`

- `manager.rs:L385`: `self.create_internal(password, "root", PwidLevel::Root.as_u8(), &all_caps)`
- v4 已经废弃了 Root 概念——CapabilityMatrix 是唯一权限来源
- 这里继续使用 `PwidLevel::Root` 更像是 v3 遗留代码

### #28. `genesis_init` 和 `GENESIS_REQUESTED` 从未被调用

- `ffi.rs:L44`: `static GENESIS_REQUESTED: AtomicBool` — 声明了
- `ffi.rs:L669`: 设置为 `true` — 在某次调用中
- 但整个代码库中没有一个函数读取或检查这个标志的实际值

---

## 完整问题清单

| # | 级别 | 文件 | 行号 | 简述 |
|---|:--:|------|------|------|
| 1 | 🔴 | ffi.rs | L749-L755 | 未注册 PWID 全通 |
| 2 | 🔴 | session.rs | L128 | 密码比较非常数时间 |
| 3 | 🔴 | manager.rs | generate() | 密码哈希不加盐 |
| 4 | 🔴 | manager.rs | generate() | PWID 只有 60 位熵 |
| 5 | 🟠 | manager.rs | L345 | can_modify() 永真 |
| 6 | 🟠 | permission.rs | L100-L101 | 信任等级检查空体 |
| 7 | 🟠 | token.rs | L290 | 紧缩保留过期 token |
| 8 | 🟠 | session.rs | elevate() | 提权栈无锁 |
| 9 | 🟡 | ffi.rs | L19-L41 | 单例 TOCTOU |
| 10 | 🟡 | manager.rs | 多处 | 锁获不一致 |
| 11 | 🔵 | storage.rs | 全文 | 持久化缺失 |
| 12 | 🔵 | session.rs | — | 无超时清理 |
| 13 | 🔵 | token.rs | L196-L199 | 满表静默失败 |
| 14 | 🔵 | ffi.rs | L681-L767 | 信任关系 Stub |
| 15 | 🔵 | audit.rs + manager | 多处 | 审计不完整 |
| 16 | 🟢 | audit.rs | L44-L48 | Race condition |
| 17 | 🟢 | token.rs | — | ID 回卷 |
| 18 | 🟢 | trust_chain.rs | L194 | 递归无栈保护 |
| 19 | 🟢 | context.rs | L221 | 风险评分反向 |
| 20 | 🟢 | capability.rs | — | 常量散落两处 |
| 21 | 🟢 | ffi.rs | L651 | 空密码 elevate |
| 22 | 🟢 | ffi.rs | L44 | GENESIS 未消费 |
| **23** | **🔴** | **ffi/hvfs/ramfs** | **多处** | **"零能力=全通"语义反置** |
| 24 | 🟠 | syscall.c | 12处 | 管理操作全或无 |
| 25 | 🔴 | main.c + ffi.rs | — | 自举问题 |
| 26 | 🔴 | storage.rs | 全文 | 重启丢失=自举循环 |
| 27 | 🟠 | manager.rs | L385 | 遗留 Root 调用 |
| 28 | 🟢 | ffi.rs | L44+L669 | GENESIS 未消费 |
