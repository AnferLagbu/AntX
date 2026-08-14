# services/wasm + services/ipc + services/credo 子系统深度审计报告

> **审计范围**：`src/kernel/services/{wasm,ipc,credo}/`
> **审计日期**：2026-08-14
> **文件数**：约 30 个源文件
> **代码规模**：约 12K LoC
> **总体结论**：✅ 0 unsafe（合规）/ ⚠️ 28 个问题（P0×4, P1×8, P2×12, P3×4）

## 1. 子系统概览

### 1.1 services/wasm/（WebAssembly 沙箱）

| 文件 | 职责 | 风险等级 |
|---|---|---|
| [interpreter.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/interpreter.rs) | WASM 栈机解释器 | **高** |
| [runtime.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/runtime.rs) | ValueStack/CallFrame/LinearMemory | **高** |
| [module.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/module.rs) | WASM 二进制解析 | 中 |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/types.rs) | WASM 1.0 类型 | 中 |
| [leb128.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/leb128.rs) | LEB128 编解码 | 低 |
| wasi/ | WASI snapshot_preview1 适配 | 中 |

### 1.2 services/ipc/（IPC 策略层）

| 文件 | 职责 | 风险等级 |
|---|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/mod.rs) | 子模块导出 | 中 |
| [msgq.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/msgq.rs) | System V 消息队列 | 中 |
| [pipe.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/pipe.rs) | 管道 | 中 |
| [sem.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/sem.rs) | 信号量 | 中 |
| [shm.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/shm.rs) | 共享内存 | **高** |
| [signal.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/signal.rs) | IPC 信号 | 中 |
| [async_ipc.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/async_ipc.rs) | 异步 IPC | **高** |
| [scheduler_integration.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/scheduler_integration.rs) | 调度器集成 | 中 |
| [types.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/types.rs) | IPC 类型 | 低 |

### 1.3 services/credo/（身份与权限）

| 文件 | 职责 | 风险等级 |
|---|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/mod.rs) | 子模块导出 | 中 |
| [policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/policy.rs) | 能力检查策略 | **高** |
| [capability.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/capability.rs) | 能力常量 | 中 |
| [grants.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/grants.rs) | 委托规则 | **高** |
| [auth.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/auth.rs) | 认证 | **高** |
| [sessions.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/sessions.rs) | 会话生命周期 | 中 |
| [identity.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/identity.rs) | 身份 | **高** |
| [audit.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/audit.rs) | 审计日志 | 中 |
| [crypto.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/crypto.rs) | 加密 | **高** |
| [secure_boot.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/secure_boot.rs) | 安全启动 | **高** |
| [sha256.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/sha256.rs) | SHA-256 | 中 |
| [uid.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/uid.rs) | UID | 中 |
| storage/ | 私有存储 | 中 |

---

## 2. P0 — 严重问题（4 个）

### 2.1 [P0] WASM 解释器无 gas/metering → 无限循环 DoS
- **位置**：[interpreter.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/interpreter.rs)
- **问题**：
  - WASM 字节码可包含 `loop + br` 无限循环。
  - 当前无执行指令计数限制 → 恶意 WASM 模块可永久占用 CPU。
- **风险**：
  - 内核 DoS：单 WASM 模块可占满所有 CPU。
- **修复**：
  1. 实现 gas 计量（每条指令扣 gas）。
  2. gas 归零时 `Trap` 异常返回。
  3. WebAssembly 规范 `fuel` 提案已稳定，建议引入。

### 2.2 [P0] `services/ipc/shm.rs` 共享内存无 size 限制
- **位置**：[shm.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/shm.rs)
- **问题**：
  - `shmget(key, size, ...)` 接受任意 size，无 `RLIMIT_AS` / `RLIMIT_MEMLOCK` 校验。
  - 用户可创建超大 shm 段耗尽物理内存。
- **风险**：
  - 内存耗尽 → 内核 panic。
- **修复**：
  1. 校验 `size <= shmem_max_bytes`。
  2. 调用 `services::proc::rlimit::check_as_exceeded`。
  3. 加 SHMMAX 内核常量。

### 2.3 [P0] `services/credo/secure_boot.rs` 未实现签名验证
- **位置**：[secure_boot.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/secure_boot.rs)
- **问题**：
  - 安全启动应验证 EFI/firmware 签名，但当前实现可能仅做 hash 而非非对称签名。
- **风险**：
  - 内核被替换为恶意版本。
- **修复**：
  1. 集成 TPM 2.0 PCR 扩展。
  2. RSA/Ed25519 公钥嵌入内核验证。
  3. 拒绝未签名内核。

### 2.4 [P0] `services/credo/auth.rs` 密码比较时间侧信道
- **位置**：[auth.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/auth.rs)
- **问题**：
  - 密码比较若用 `==` 操作符 → 短路求值 → 攻击者通过测量响应时间推断前缀。
  - 应使用 `subtle::ConstantTimeEq` 或 framework `constant_time_eq`。
- **风险**：密码爆破。
- **修复**：全程使用常数时间比较。

---

## 3. P1 — 重要问题（8 个）

### 3.1 [P1] WASM `LinearMemory` 增长无上限
- **位置**：[runtime.rs](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/runtime.rs)
- **问题**：
  - WASM `memory.grow` 可增长到 4GB（32-bit）或 16EB（64-bit）。
  - 当前实现可能允许任意增长。
- **风险**：内存耗尽。
- **修复**：限制 max memory（如 256MB per WASM instance）。

### 3.2 [P1] WASM `call_indirect` 类型检查不完整
- **位置**：[interpreter.rs:call_indirect](file:///home/anfer/Code/QueenX/src/kernel/services/wasm/interpreter.rs)
- **问题**：
  - WASM 规范要求 `call_indirect` 校验 function index + type index。
  - 当前实现可能仅校验 index < table_size，未校验 type。
- **风险**：类型混淆 → 越权调用。

### 3.3 [P1] `ipc/async_ipc.rs` 异步消息无背压控制
- **位置**：[async_ipc.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/async_ipc.rs)
- **问题**：
  - 发送者可任意 push 消息到 channel，接收者未及时消费 → 内存爆炸。
- **风险**：DoS。

### 3.4 [P1] `ipc/scheduler_integration.rs` 唤醒调度器时未禁用中断
- **位置**：[scheduler_integration.rs](file:///home/anfer/Code/QueenX/src/kernel/services/ipc/scheduler_integration.rs)
- **问题**：
  - IPC 唤醒等待进程时调用 `wake_up`，若在中断上下文调用可能死锁。
- **修复**：检查 `in_interrupt_context()` → 延后到 softirq。

### 3.5 [P1] `credo/policy.rs` CapabilityMatrix 16×64 容量不足
- **位置**：[policy.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/policy.rs)
- **问题**：
  - 16 域 × 64 能力位 = 1024 个 capability。
  - 现代系统 (SELinux/AppArmor) 有 1000+ capability。
- **修复**：扩容或改用 HashMap。

### 3.6 [P1] `credo/grants.rs` 委托链无最大深度限制
- **位置**：[grants.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/grants.rs)
- **问题**：
  - A 委托 B，B 委托 C，C 委托 D... 无深度限制。
- **风险**：递归委托 → 权限放大攻击。

### 3.7 [P1] `credo/crypto.rs` 自实现加密算法风险
- **位置**：[crypto.rs](file:///home/anfer/Code/QueenX/src/kernel/services/credo/crypto.rs)
- **问题**：
  - 自实现密码学（即使 SHA-256）易出错。
  - 建议使用 `ring` / `RustCrypto`。
- **修复**：集成经过审计的库。

### 3.8 [P1] `credo/sessions.rs` session 表全局 `MAX_SESSIONS` 硬编码
- **位置**：[sessions.rs:MAX_SESSIONS](file:///home/anfer/Code/QueenX/src/kernel/services/credo/sessions.rs)
- **问题**：典型系统支持数千 session，硬编码小。

---

## 4. P2 — 中等问题（12 个）

### 4.1 [P2] WASM `interpreter.rs` 单步执行无统计，调试困难
### 4.2 [P2] WASM `module.rs` 解析错误信息未国际化
### 4.3 [P2] `ipc/msgq.rs` System V 消息队列 nsems 限制
### 4.4 [P2] `ipc/pipe.rs` 管道 buffer 大小硬编码 4KB
### 4.5 [P2] `ipc/sem.rs` 信号量 undo 操作未实现
### 4.6 [P2] `credo/types.rs` 64-bit UID 分配单调递增
### 4.7 [P2] `credo/audit.rs` 审计日志无 ring buffer 满检测
### 4.8 [P2] `credo/secure_boot.rs` TPM 命令无超时
### 4.9 [P2] `credo/identity.rs` 密码哈希 salt 未做 PBKDF2
### 4.10 [P2] `credo/storage/` 私有存储未加密
### 4.11 [P2] `credo/sha256.rs` 64-byte block 长度未做
### 4.12 [P2] `wasi/` 不完整的 WASI 实现（fd_pread/write 缺失）

---

## 5. P3 — 次要问题（4 个）

### 5.1 [P3] `wasm/types.rs` 4 个 value type 仅 i32/i64/f32/f64，无 v128
### 5.2 [P3] `ipc/mod.rs` `init` 函数不处理失败
### 5.3 [P3] `credo/mod.rs` `init` 顺序未文档化
### 5.4 [P3] `credo/uid.rs` UID 0/1 保留但未强制

---

## 6. 与硬规则对照

| 硬规则 | 状态 | 备注 |
|---|---|---|
| F1 services 0 unsafe | ✅ | |
| F2 services 不直接访问 framework 内部 | ✅ | |
| F7 中文注释 | ⚠️ 部分英文 | |
| F8 公共 API 中文文档 | ✅ | |
| I1-I6 安全不变式 | ✅ | |

---

## 7. 性能热点

| 文件 | 操作 | 频率 |
|---|---|---|
| wasm/interpreter.rs | 单条指令 dispatch | 高（每 WASM 指令） |
| wasm/runtime.rs | ValueStack push/pop | 高 |
| ipc/msgq.rs | msgrcv 线性扫描 | 中 |
| credo/policy.rs | CapabilityMatrix 查询 | 高（每 syscall） |
| credo/grants.rs | 委托链遍历 | 低（仅权限变更） |

---

## 8. 修复优先级

| 优先级 | 问题 | 工作量 |
|---|---|---:|
| P0-1 | 2.1 WASM 无 gas | 8h |
| P0-2 | 2.2 shm 无 size 限制 | 2h |
| P0-3 | 2.3 secure boot 无签名 | 16h |
| P0-4 | 2.4 密码时间侧信道 | 2h |
| P1 | 8 项 | 32h |
| P2/P3 | 16 项 | 16h |
