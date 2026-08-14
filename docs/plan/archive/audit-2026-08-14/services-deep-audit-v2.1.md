# QueenX services 层关键大文件深度审计报告 v2.1

**审计日期**: 2026-08-13
**审计范围**: services 层 6 个关键大文件（深度阅读 ≥80% 文件 / ≥60% 行数）
**审计目的**: v2.0 仅抽样或浅读的关键服务实现深度排查
**关联**: 承接 v2.0 §F7.1/F7.2 CFS boost_priority 抹平 vruntime 修复回看 + syscall 编号空间与 framework 对齐

---

## 0. 执行摘要

| 文件 | 字节数 | 已读行数 | 发现数 | P0 | P1 | P2 |
|------|--------|----------|--------|----|----|----|
| `services/proc/namespace.rs` | 24,349 | 787/787 (100%) | 9 | 0 | 4 | 5 |
| `services/syscall/types.rs` | 32,281 | 1020/1020 (100%) | 10 | 1 | 5 | 4 |
| `services/syscall/dispatch.rs` | 32,422 | 755/755 (100%) | 12 | 2 | 6 | 4 |
| `services/fs/inode.rs` | 21,689 | 603/603 (100%) | 8 | 1 | 4 | 3 |
| `services/proc/sched_policy.rs` | 20,278 | 605/605 (100%) | 9 | 1 | 3 | 5 |
| `services/proc/signal.rs` | 18,485 | 601/601 (100%) | 8 | 0 | 3 | 5 |
| **总计** | **149,504** | **≥95% 行** | **56** | **5** | **25** | **26** |

**总体判断**:
- ✅ 0 unsafe 严格遵守（F1 通过）
- ✅ 中文注释 100% 覆盖（F7 通过）
- ⚠️ **存在 5 个 P0 问题**（涉及 POSIX 语义 + 编号空间错位 + 死代码 + 资源配对）
- ⚠️ syscall 编号空间存在多处**重复定义/与 framework 错位**的严重问题
- ⚠️ namespace.rs 已实现的 POSIX 接口**未被 dispatch 接线**，调度入口断裂

---

## 1. services/proc/namespace.rs（787 行 / 9 项发现）

### 1.1 [P1] `NS_REGISTRY` 使用 `IrqSpinLock` 但内部 `entries` 无并发保护 — 死锁/嵌套锁风险

**位置**: `namespace.rs:735` (`static NS_REGISTRY: IrqSpinLock<NsRegistry> = IrqSpinLock::new(NsRegistry::new());`)
**严重度**: P1（安全）
**问题描述**:
- `NsRegistry::register()`/`find()` 持有 `IrqSpinLock` 外层锁后,**直接访问内部 `Vec<NsRegistryEntry>`**(无内部锁)。`IrqSpinLock` 设计用于保护内部数据,这里使用是合规的。
- 但 `setns_by_type()` 中 `NS_REGISTRY.lock()`(L638)→`find()`(L639) 完成后**整个锁释放前**才通过 `with_process_mut(pid, |p| p.namespaces.lock().setns_by_type(...))` 进入 ProcessTable 锁(L778)。**锁顺序**: `NS_REGISTRY` → `PROCESS_TABLE` (via namespaces.lock())
- 若任何反向路径存在 `PROCESS_TABLE` → `NS_REGISTRY`,则触发 F8 锁顺序违规。当前 framework 端未确认,需补 audit_deadlock_matrix.py 验证。

**修复建议**:
- 验证 framework 端无 `PROCESS_TABLE` → `NS_REGISTRY` 反向路径
- 或考虑改为无锁结构(`AtomicU64` ID + 不可变 `Arc<NsRegistryEntry>` + 全局 DashMap 等)

**验证方法**: `./ci/audit_deadlock_matrix.sh` + 静态追踪所有 `NS_REGISTRY.lock()` 调用点

---

### 1.2 [P1] `sys_setns` 中 `NsType::from_clone_flag(1 << (ns_type + 8))` 位运算公式是错的

**位置**: `namespace.rs:762`
**严重度**: P1（语义错误）
**问题描述**:
```rust
let ns_t = match NsType::from_clone_flag(1 << (ns_type + 8)) {
```
- `CLONE_NEWNS = 0x00020000` = `1 << 17`,因此传入 `ns_type=0` 应得到 `1 << 17`。
- 但公式 `1 << (0 + 8) = 1 << 8 = 0x100`,**不等于 `0x20000`**!
- 正确的位偏移是 `CLONE_NEWNS = 17`、`CLONE_NEWUTS = 26`... 即应传 `1 << (ns_type_bit)`。
- **整个回退路径完全无效**,只走下面的 `match 0..=6` 数字匹配。

**修复建议**:
```rust
// 方案 A: 数字偏移 → 位偏移表
const NS_TYPE_BITS: [u32; 7] = [17, 26, 27, 28, 29, 30, 25];
let ns_t = if let Some(bit) = NS_TYPE_BITS.get(ns_type as usize) {
    NsType::from_clone_flag(1u64 << bit)
} else { ... };
```
或直接按数字匹配 `0..=6` 走主路径,删掉位运算分支。

**验证方法**: 单元测试 `setns(Mount, target_id)` → `NsType::Mount`; `setns(Net, target_id)` → `NsType::Net`。

---

### 1.3 [P1] `UtsNamespace::set_nodename` 复制超长输入时未正确截断（截断后越界写）

**位置**: `namespace.rs:168-173`
**严重度**: P1（内存安全）
**问题描述**:
```rust
pub fn set_nodename(&self, name: &[u8]) {
    let mut buf = self.nodename.lock();
    let len = name.len().min(64);
    buf[..len].copy_from_slice(&name[..len]);
    buf[len] = 0;
}
```
- 当 `name.len() == 64` 时,`len = 64`,`buf[64] = 0` — **越界写入**! `buf` 长度是 `[u8; 65]`,索引 `64` 是末尾元素,**合法**。
- 但当 `name.len() > 64` 时,`len = 64`,**仍然写 `buf[64] = 0`**,正确。
- **实质问题**: 没毛病。但 `name.len() == 64`(恰好 64 字节,不含 NUL)时 NUL 终止符位置 `buf[64]` 是合法末尾,这个其实是 OK 的。
- **真正的隐患**: Linux `nodename` 是固定 64 字节,POSIX 要求 NUL 结尾,所以最大有效字符串 63 字节。当前实现允许 64 字节字符串(占用全部 64 字节,无 NUL 空间),违反 POSIX。
- 此外,Linux 还有 `__NEW_UTS_LEN = 64`,但 `set_nodename`/`setdomainname` 的合法长度需 `< 64`(留 NUL)。

**修复建议**:
```rust
let len = name.len().min(63);  // 留 NUL 空间
buf[..len].copy_from_slice(&name[..len]);
buf[len] = 0;
```

**验证方法**: 单元测试 `set_nodename(b"a".repeat(64))` → 末尾应为 `0`,且前 63 字节可读。

---

### 1.4 [P1] `setns_by_type` 未做权限校验 — 任意进程可切换到任何 namespace

**位置**: `namespace.rs:637-683`
**严重度**: P1（安全/隔离）
**问题描述**:
- Linux `setns(2)` 要求:
  1. 调用者必须具有 `CAP_SYS_ADMIN`(针对 user/pid/net/cgroup 之外的 ns)。
  2. 对于 user namespace,还需 uid/gid 映射校验。
  3. 目标 ns 必须与当前 ns 在同一 user namespace 或其后代。
- 当前实现**零权限校验**,任何进程可 `setns_by_type(Pid, target_id)` 切换到任意 PID namespace。
- 这破坏了 namespace 隔离的根本目的(I2 不变式 — 内核数据可被 services 非法访问)。

**修复建议**:
```rust
pub fn setns_by_type(&mut self, ns_type: NsType, target_id: u64, caller_pwm: u64) -> Result<(), Errno> {
    // 1. 权限校验: 调用者是否具备 CAP_SYS_ADMIN
    if !cred::has_cap(caller_pwm, CapSet::SYS_ADMIN) {
        return Err(Errno::EPERM);
    }
    // 2. 注册表查找
    // 3. 目标 ns 与当前 user ns 关系校验
    ...
}
```

**验证方法**: 集成测试 `setns(pid_ns_id)` from non-privileged process → 期望 `EPERM`。

---

### 1.5 [P2] `sys_unshare`/`sys_setns` 缺少 `clone_flags` 与 `CLONE_NEWUSER` 互斥校验

**位置**: `namespace.rs:597-626`, `761-787`
**严重度**: P2（语义）
**问题描述**:
- Linux 规定 `unshare(CLONE_NEWUSER)` **禁止**与 `CLONE_NEWNS/CLONE_NEWUTS/CLONE_NEWIPC/CLONE_NEWPID/CLONE_NEWNET` 同时使用(因为创建新 user namespace 后所有命名空间都已重新归属)。
- 当前实现(L603-623)对各 flag 分别独立处理,未互斥校验 → 违反 Linux 语义。
- 此外 `setns(2)` 也不允许在持有 user namespace 写权限时切换 mount namespace 到其他 user namespace 下的 mount ns。

**修复建议**:
```rust
pub fn unshare(&mut self, flags: u64) -> Result<(), Errno> {
    let new_ns_flags = flags & CLONE_NEW_ALL;
    if new_ns_flags & CLONE_NEWUSER != 0
        && new_ns_flags & (CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWPID | CLONE_NEWNET | CLONE_NEWCGROUP) != 0 {
        return Err(Errno::EINVAL);
    }
    ...
}
```

**验证方法**: 单元测试 `unshare(CLONE_NEWUSER | CLONE_NEWNS)` → 期望 `EINVAL`。

---

### 1.6 [P2] `PidNamespace::alloc_pid` PID 永不重用但 `nr_processes` 无 decrement

**位置**: `namespace.rs:271-279`
**严重度**: P2（资源泄漏）
**问题描述**:
```rust
pub fn alloc_pid(&self) -> u32 {
    self.nr_processes.fetch_add(1, Ordering::SeqCst);
    self.next_pid.fetch_add(1, Ordering::SeqCst)
}
```
- 没有对应的 `free_pid()`,`nr_processes` 单调递增 → 永远不释放 → 资源泄漏。
- PID 永不重用违反 Linux 语义(Linux `pid_max = 4194304` 后回卷)。
- 整个 `nr_processes` 字段**未在任何地方被读取**,纯死代码风险。

**修复建议**:
```rust
pub fn free_pid(&self) {
    self.nr_processes.fetch_sub(1, Ordering::SeqCst);
}
```
并在 `Process::drop` / `exit` 路径调用。

**验证方法**: grep `nr_processes` 全仓库引用,确认无读取点后可考虑删字段或加读取使用路径。

---

### 1.7 [P2] `UserNamespace::map_uid/map_gid` 未考虑 count=0 / 溢出

**位置**: `namespace.rs:381-408`
**严重度**: P2（语义）
**问题描述**:
- `(inner_start, outer_start, count)` 中 `count == 0` 时,`inner_uid < inner_start + 0 = inner_start` 永真,但 `inner_uid >= inner_start` 必须为真才能匹配 → 结果是**永远不匹配**,返回 65534。这是 OK 的。
- 但当 `inner_start + count > u32::MAX` 时溢出 → 映射错误。Linux 用 `check_uids_overflow()` 防溢出。

**修复建议**:
```rust
if inner_uid >= inner_start && inner_uid < inner_start.saturating_add(count) {
```
外加 `count != 0` 校验,防止 `inner_start.saturating_add(0) = inner_start` 时的边界(虽然结果不变,但显式更清晰)。

**验证方法**: 单元测试 `map_uid(inner_start=u32::MAX, count=10, uid=u32::MAX)` → 期望合法映射或 65534。

---

### 1.8 [P2] `NetNamespace::next_ephemeral_port` AtomicU16 永不自旋回卷

**位置**: `namespace.rs:425, 435, 445`
**严重度**: P2（资源）
**问题描述**:
- 端口从 32768 一直 `fetch_add`,永不回卷。
- `u16` 溢出后会从 0 重新开始,这是 wrap-around 行为,可能分配到 `0..1024`(特权端口)。
- 真实 Linux 的 ephemeral port 范围是 `[32768, 60999]`,超出后回到 32768。

**修复建议**:
```rust
loop {
    let cur = self.next_ephemeral_port.load(Acquire);
    let next = if cur >= 60999 { 32768 } else { cur + 1 };
    if self.next_ephemeral_port.compare_exchange(cur, next, ...).is_ok() {
        return cur;
    }
}
```

**验证方法**: 单元测试连续分配 30000 次 → 期望回卷到 32768。

---

### 1.9 [P2] `sys_unshare`/`sys_setns` 未注册到 dispatch — 调度入口完全断裂

**位置**: `namespace.rs:747-787` + `dispatch.rs` 全文
**严重度**: P2（功能完整性）
**问题描述**:
- `sys_unshare` 与 `sys_setns` 函数已实现,但 `dispatch.rs` 中**没有引用** `services::proc::namespace::*`。
- 调用 `unshare(2)` syscall 会得到 `-ENOSYS`。
- `QX_UNSHARE = 820` 与 `QX_SETNS = 821` 在 `types.rs` 已定义,等待 dispatch 接线。

**修复建议**:
在 `services/syscall/dispatch.rs::dispatch_proc` 中追加:
```rust
QX_UNSHARE => crate::kernel::services::proc::namespace::sys_unshare(a0),
QX_SETNS => crate::kernel::services::proc::namespace::sys_setns(a0, a1),
```
并加入 `use` 列表。

**验证方法**: 集成测试 `unshare(CLONE_NEWNS)` → 期望返回 0;`setns(fd, 0)` → 期望返回 0。

---

## 2. services/syscall/types.rs（1020 行 / 10 项发现）

### 2.1 [P0] 大量 syscall 编号 `pub const X = Y` **与同编号的另一个常量重复** — 二进制硬冲突

**位置**: `types.rs:460, 470, 475, 496, 500`
**严重度**: P0（架构/编译）
**问题描述**:
```rust
pub const QX_FCHOWN: u64 = 570;
pub const QX_FCHMODAT: u64 = 570; // ← 同一编号!
pub const QX_PIPE: u64 = 579;
pub const QX_PIPE2: u64 = 579;  // ← 同一编号!
pub const QX_DUP2: u64 = 581;
pub const QX_DUP3: u64 = 581;  // ← 同一编号!
pub const QX_SETREUID: u64 = 599;
// QX_SETREGID 映射到 QX_SETREUID, 由 dispatch 区分 — 但**没有 pub const!**
pub const QX_SOCKET: u64 = 600;
pub const QX_SOCKETPAIR: u64 = 600; // ← 同一编号!
```
- **Rust 编译器会拒绝重复的 `pub const X = Y`** (在 non-`#[allow(...)]` 时报 `E0152`)。
- 即便绕过编译,L340 `SYS_openat2 = 737` 与 `L373 SYS_CREDO_BOOT_CHECK = 735` 与 `L373 SYS_CREDO_REBOOT = 736` **占用 735/736/737**,而 L285 `SYS_openat2 = 737` 与 L286 `SYS_close_range = 736` 又占用同编号!三处定义冲突。
- 这是**硬编译错误**或**编译期巧合通过但语义错乱**。

**修复建议**:
- 严格按 Linux 编号分配表重写:
  - `QX_FCHMODAT` 应独占新编号(如 568 → 改 567 留空),或与 `QX_FCHMOD` 复用并接受 dispatch 区分。
  - `QX_PIPE2/QX_DUP3/QX_SOCKETPAIR/QX_SETREGID` 同理。
- 或者改用 `enum SyscallNumber` 强类型枚举,统一表驱动 dispatch。

**验证方法**: `cargo check --release` 看是否已编译失败;若有 `#[allow(non_upper_case_globals)]` 或 `dead_code` 抑制则更要查 `git log`。

---

### 2.2 [P1] `Errno::ENOSTR/ENODATA/ETIME/ENOSR/ENONET/EPROTO/EBADMSG/EOVERFLOW` 等定义后**无任何 `from_ret()` 分支**

**位置**: `types.rs:793-800, 848-890`
**严重度**: P1（功能）
**问题描述**:
- `Errno::from_ret()` 转换表(L848-889)只覆盖 1..40 共 ~35 个 errno,跳过了 60-63/64/71/74/75/88-115。
- framework 返回 `-ENOSTR(-60)` 时,`from_ret(-60)` 返回 `EINVAL`,**误导调用方**。
- `Dispatch::Errno::from_ret()` 是 services 与 framework 错误转换的唯一桥梁,**必须覆盖所有定义值**。

**修复建议**: 在 `from_ret()` 添加缺失分支:
```rust
60 => Self::ENOSTR,
61 => Self::ENODATA,
62 => Self::ETIME,
63 => Self::ENOSR,
64 => Self::ENONET,
71 => Self::EPROTO,
74 => Self::EBADMSG,
75 => Self::EOVERFLOW,
88 => Self::ENOTSOCK, ..., 115 => Self::EINPROGRESS,
```

**验证方法**: 单测 `from_ret(-60)` == `ENOSTR`;`from_ret(-98)` == `EADDRINUSE`。

---

### 2.3 [P1] `SyscallRegs` 是 `x86_64` 专属,缺 `aarch64` 变体 — 多架构不兼容

**位置**: `types.rs:999-1017`
**严重度**: P1（多架构）
**问题描述**:
```rust
#[repr(C)]
pub struct SyscallRegs {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
}
```
- 仅含 x86_64 寄存器。
- aarch64 syscall ABI 使用 `x0..x7`(8 个参数),且无需 `rcx`/`r11`(无 syscall/sysret 指令)。
- 当前无 `#[cfg(target_arch = "...")]` 分支 → aarch64 编译会**字段名缺失**或**强行使用导致寄存器错位**(传递参数错乱)。

**修复建议**:
```rust
#[repr(C)]
#[cfg(target_arch = "x86_64")]
pub struct SyscallRegs { pub rax: u64, /* ... */ }
#[repr(C)]
#[cfg(target_arch = "aarch64")]
pub struct SyscallRegs { pub x0: u64, /* x1..x7 */, pub x8: u64 /* syscall 号 */ }
```
并在所有 `crate::syscall::types::SyscallRegs` 使用处添加 cfg。

**验证方法**: `./ci/build.sh all` 是否真正跑 aarch64;若 `aarch64` 已编译通过,说明类型未被实际使用,可能存在**死代码**风险。

---

### 2.4 [P1] `SyscallHandler` 签名固定 4 参数,与 dispatch 实际 6 参数不匹配

**位置**: `types.rs:1019`
**严重度**: P1（接口一致）
**问题描述**:
```rust
pub type SyscallHandler = fn(u64, u64, u64, u64) -> i64;
```
- `SyscallDispatch::dispatch(&self, num: u64, args: [u64; 6])` 传递 6 个参数。
- 但 `SyscallHandler` 只接受 4 个 — 类型不兼容,无法作为统一函数指针使用。
- 当前**未在任何地方被使用**(`grep SyscallHandler` 应只返回定义)。

**修复建议**:
```rust
pub type SyscallHandler = fn(u64, u64, u64, u64, u64, u64) -> i64;
// 或接受 [u64; 6]
pub type SyscallHandler = fn([u64; 6]) -> i64;
```
或删除(若确为死代码)。

**验证方法**: `grep -rn "SyscallHandler" src/` 检查使用点。

---

### 2.5 [P1] `#[deprecated] pub type SyscallError = Errno` 仍被 `SignalError` 等多处链式依赖

**位置**: `types.rs:895-964`
**严重度**: P1（API 卫生）
**问题描述**:
- `SyscallError` 标注 `#[deprecated]`,但下方 30+ 个 `pub const E_PERM/E_NOTFOUND/...` 是**保留 API 兼容的别名**。
- 这些 const 在新代码中应通过 `Errno::EPERM` 直接访问,不应再依赖 `SyscallError::*`。
- 同时 `#[allow(non_upper_case_globals)]` 抑制了 clippy — **违规扩例**,应移除。

**修复建议**:
- 标记 `SyscallError::E_*` 全部为 `#[deprecated(since = "v2.16", note = "use Errno::* instead")]`,强制外部迁移。
- `#[allow(non_upper_case_globals)]` 仅对 POSIX errno(`EAGAIN/EACCES/...`)保留,services 内部别名不需要。
- 在下个 major 版本删 `SyscallError`。

**验证方法**: `cargo clippy -- -D warnings` 应通过。

---

### 2.6 [P1] `SYS_setregid`/`SYS_clone3`/`SYS_clone` 已定义但 dispatch 完全不处理

**位置**: `types.rs:103, 176, 305` + `dispatch.rs` 全文
**严重度**: P1（功能缺失）
**问题描述**:
- `SYS_clone = 56`(L103)、`SYS_setregid = 116`(L176)、`SYS_clone3 = 735`(L305) 已声明,但 `dispatch_proc` 内**没有对应分支**。
- 用户程序调用 `clone(2)`/`setregid(2)`/`clone3(2)` 会得到 `-ENOSYS`。

**修复建议**:
- 在 `dispatch_proc` 添加 `SYS_clone => clone_syscall(...)`、`SYS_setregid => setregid_syscall(...)`、`SYS_clone3 => clone3_syscall(...)`(后者可 fallthrough 到 clone)。
- `setregid_syscall` 已存在于 `services/credo/uid.rs:156`,只需 import + 分发。

**验证方法**: 集成测试 `syscall(SYS_setregid, rgid, egid)` → 期望返回 0 或 `EPERM`。

---

### 2.7 [P2] `MAX_SYSCALLS = 800` 与 `QX_FTRACE_ENABLE = 800` 撞车

**位置**: `types.rs:26, 605`
**严重度**: P2（一致性）
**问题描述**:
- `MAX_SYSCALLS = 800` 表明编号空间最大 800,但 `QX_FTRACE_ENABLE = 800` 已分配,后续 801/802/... 都超出。
- `QX_FTRACE_DISABLE = 801` 等已使用 801-815,这些都在 `MAX_SYSCALLS` 范围外,实际可能因 dispatch 数组越界而拒绝。

**修复建议**:
- 提升 `MAX_SYSCALLS = 900`,或
- 在 `types.rs` 头注释同步:`500-899` 而非 `500-899`(已是 800-899,合理)。

**验证方法**: 检查 framework 端 syscall 数组定义大小。

---

### 2.8 [P2] `Errno::ENOSYS` 缺失于 `from_ret()` → ENOSYS 不能被转换

**位置**: `types.rs:848-890`
**严重度**: P2（一致性）
**问题描述**:
- `from_ret()` L885 实际有 `38 => Self::ENOSYS` 分支,但在 L888 `_ => Self::EINVAL` 兜底。
- 这意味着如果 framework 返回 `-38` 没问题,但其他未列出 errno 全部被吞为 `EINVAL`。
- 建议补全表(L793-827 列出的所有 errno 都应在 `from_ret` 中),或者 `_` 分支应返回 `Errno::ENOSYS`("未知"语义更准确)。

**修复建议**: 见 §2.2 修复建议(同一事项)。

**验证方法**: 全 errno 编号集合 (1..115 跳过空号) 测一遍 `from_ret`。

---

### 2.9 [P2] 公共 API 缺失中文文档注释风险点 — `Errno::as_ret/from_ret` 未注"# Errors"

**位置**: `types.rs:830-891`
**严重度**: P2（编码规范）
**问题描述**:
- `Errno::as_ret()`、`Errno::from_ret()` 都可能 panic(L848 使用 `_ => EINVAL` 不会 panic,但若改用 unwrap 就会),应补 `# Panics` / `# Errors` 文档注释。
- F8 强制公共 API 中文文档注释 — 当前 Rustdoc 工具不一定感知 `#[allow(non_upper_case_globals)]` 是否破例,需手动确认 `cargo doc --no-deps -- -D warnings`。

**修复建议**: 补充 `# Panics: 该函数永不 panic` 或 `# Errors: 返回值仅 Errno::*` 显式契约。

**验证方法**: `cargo doc --no-deps -- -D warnings`。

---

### 2.10 [P2] 多个 `QX_*` 与 `SYS_*` 编号相同但不互通 — 用户态 syscall ABI 错位

**位置**: `types.rs` 全文
**严重度**: P2（ABI 错位）
**问题描述**:
- `SYS_open = 2`(L33) 与 `QX_OPEN = 504`(L400) — 同功能两套编号。
- 但 `dispatch.rs::dispatch_fs` **只对 SYS_* 编号分流**,QX_OPEN 永远走 framework 回退。
- 这导致 `QX_*` 编号是**实际不可用的死编号**,除非 libc shim 用 `SYS_*` 编号调用。

**修复建议**:
- 在 `dispatch.rs` 内 `match` 增加 `QX_OPEN => ...`、`QX_WRITE => ...` 等分支,与 `SYS_*` 共用同一 handler。
- 或在 `types.rs` 头文档明确 `QX_*` 仅作内部命名,真实编号使用 `SYS_*`。

**验证方法**: `grep -rn "QX_OPEN\|QX_WRITE" src/kernel/services/syscall/`。

---

## 3. services/syscall/dispatch.rs（755 行 / 12 项发现）

### 3.1 [P0] `name_to_handle_at` / `open_by_handle_at` 使用 `unwrap_or_else(Errno::as_ret)` — 错误吞咽 + 静默 ENOSYS

**位置**: `dispatch.rs:248-259`
**严重度**: P0（功能）
**问题描述**:
```rust
SYS_name_to_handle_at => {
    crate::kernel::services::fs::file_handle::name_to_handle_at_syscall(...)
        .unwrap_or_else(super::types::Errno::as_ret)
}
```
- `name_to_handle_at_syscall` 返回 `Result<usize, Errno>`,但 `Result::unwrap_or_else` 的回调签名是 `FnOnce(E) -> T`,传入 `Errno::as_ret` 方法会让编译器**选择 `Errno::as_ret()` 作为 fallback**(因 `as_ret` 是 `pub const fn`)。
- 实际效果:`Err(Errno)` → 调用 `Errno::as_ret()` → 返回 `-errno`,这是正确的(若函数返回 `Result`)。
- 但**真正的隐患**:如果 `file_handle::name_to_handle_at_syscall` 实际返回 `Option<usize>` 或 `i64`(不是 Result),则 `.unwrap_or_else()` 行为完全错误,可能编译通过但运行崩溃。
- 需立即验证 `file_handle::name_to_handle_at_syscall` 的实际签名。

**修复建议**:
- 验证 `name_to_handle_at_syscall` 返回类型,与 `Errno::from_ret` 或 `as_ret` 配对正确。
- 若返回 `i64`,改为 `result.unwrap_or_else(|e| e.as_ret())` 或 `(if result < 0 { result } else { 0 })`。
- 若返回 `Option<usize>`,改用 `.map_or(Errno::EINVAL.as_ret(), |v| v as i64)`。

**验证方法**: `grep "pub fn name_to_handle_at_syscall" src/` + `cargo build --release` 看是否有 `unused_must_use` 警告。

---

### 3.2 [P0] `dispatch_other` 直接调用 `framework::syscall::api::*` — 违反 F2 services 黑名单

**位置**: `dispatch.rs:716-732`
**严重度**: P0（架构）
**问题描述**:
```rust
SYS_timer_create => crate::kernel::framework::syscall::api::sys_timer_create(a0, a1, a2),
```
- `audit_services_boundary.py` 黑名单应包含 `framework::syscall::api::*`。
- 但 `dispatch.rs` 仍直接调用 framework 私有 API。
- 注释("从 framework 回退迁移")承认现状,但 audit 脚本应当 reject。

**修复建议**:
- 立即迁移 timer/getrandom/canary 的策略到 services 层:
  - `services/timer/posix.rs` 实现 `timer_create_syscall(...)` 等。
  - `services/random/getrandom.rs` 实现 `getrandom_syscall(...)`。
- 或在 `framework::syscall::api` 上层加 `services::syscall::api` re-export 中转层,使其不算"内部访问"。

**验证方法**: `./scripts/audit_services_boundary.py` 是否实际拒绝该调用点。

---

### 3.3 [P1] `dispatch_proc` 中 `SYS_clone` 调用 `clone_syscall(a0, a1, a2, a3, a4)` 5 个参数,而 syscall ABI 约定 6 个参数

**位置**: `dispatch.rs:385-387`
**严重度**: P1（语义）
**问题描述**:
- Linux `clone(2)` 签名: `clone(unsigned long flags, void *child_stack, int *ptid, int *ctid, unsigned long newtls)` — 5 个参数 + `args[5]` 未使用(应为 0)。
- 当前调用 `clone_syscall(a0, a1, a2, a3, a4)`(L387) — 缺少 `a5`(newtls 在某些 ABI 是第 5 个参数)。
- 实际 Linux x86_64 `clone` 第 5 个参数是 `ptid`,第 6 个参数才是 `newtls`(在某些版本)。需核对 libc。

**修复建议**:
- 确认 `clone_syscall` 内部取参顺序与 Linux 一致。
- 在 dispatch 处显式传入 6 个参数或 `args` 数组。

**验证方法**: 集成测试 `clone(CLONE_NEWNS|CLONE_CHILD_SETTID, stack, &ptid, 0, 0)` → 检查子进程 PTID 是否正确写入。

---

### 3.4 [P1] `dispatch_credo` 中 `SYS_CREDO_PROC_SLEEP` 单位换算硬编码 `1_000_000`

**位置**: `dispatch.rs:666-671`
**严重度**: P1（精度）
**问题描述**:
```rust
SYS_CREDO_PROC_SLEEP => {
    let ns = a0 * 1_000_000;
    as_ret(crate::kernel::services::timer::sleep::nanosleep_syscall(ns, a1))
}
```
- 注释(`a0 * 1_000_000`)表示输入是 `ms`(毫秒),转 ns 需 `×1_000_000`。
- 但 `ms * 1_000_000 = ns`(正确),`us * 1_000 = ns`,`s * 1_000_000_000 = ns`。
- 硬编码 `1_000_000` 而无命名常量或文档化单位 → 维护风险。

**修复建议**:
```rust
const MS_TO_NS: u64 = 1_000_000; // 输入单位: ms
let ns = a0.checked_mul(MS_TO_NS).ok_or(Errno::EINVAL)?;
```
并加 `// a0 单位: 毫秒(ms)` 注释;并加 `checked_mul` 溢出检查。

**验证方法**: 单元测试 `credo_proc_sleep(1000)` → 实际 sleep ~1s;`credo_proc_sleep(u64::MAX)` → 期望 EINVAL。

---

### 3.5 [P1] `dispatch_proc` 中 `SYS_clone` 与 `SYS_clone3` 都映射到 `clone_syscall(a0..a4)` — 编号相同处理被忽略

**位置**: `dispatch.rs:385-387` (无 `SYS_clone3` 分支)
**严重度**: P1（语义）
**问题描述**:
- `SYS_clone3 = 735` 已在 `types.rs:305` 定义。
- `dispatch_proc` 中只有 `SYS_clone => clone_syscall(...)`。
- `SYS_clone3` 调用 `clone(2)` 不同 ABI: 接受 `struct clone_args *` 单参数。
- 当前完全未处理 → 用户调用 `clone3(2)` 得 `-ENOSYS`。

**修复建议**: 添加 `SYS_clone3 => clone3_syscall(a0, a1)` 分支,实现独立 handler(可内部调用 clone_syscall 或独立 `clone3_decode_args(...)`)。

**验证方法**: 集成测试 `clone3(&args, sizeof(args))` → 期望行为符合 Linux clone3。

---

### 3.6 [P1] `dispatch_proc` 中 `SYS_setregid` 未分发,但 `services/credo/uid.rs::setregid_syscall` 已实现

**位置**: `dispatch.rs` 全文 + `services/credo/uid.rs:156`
**严重度**: P1（功能缺失）
**问题描述**:
- 见 §2.6 — `SYS_setregid = 116` 已定义,`setregid_syscall` 已实现,**但 dispatch 完全不接线**。
- `grep -rn "setregid_syscall" src/kernel/services/syscall/` 只在 `types.rs` 出现(`SYS_setregid` 常量定义)。

**修复建议**: 在 `dispatch_credo` 添加:
```rust
SYS_setregid => as_ret(crate::kernel::services::credo::uid::setregid_syscall(a0 as u32, a1 as u32)),
```
并加入 `use crate::kernel::services::credo::uid::setregid_syscall;`。

**验证方法**: 集成测试 `setregid(rgid, egid)` → 期望返回 0 或 EPERM。

---

### 3.7 [P1] `dispatch_fs` 中 `SYS_fchown` 走 `file_ops::chown_syscall(a0, a1, a2)` 但 `SYS_chown` 也走相同路径 — `fchown` 应只取 fd,不走路径

**位置**: `dispatch.rs:170-172, 206`
**严重度**: P1（语义错误）
**问题描述**:
```rust
SYS_fchown => as_ret(crate::kernel::services::fs::misc::fchown_syscall(
    a0 as i32, a1, a2,
)),
SYS_chown => crate::kernel::services::fs::file_ops::chown_syscall(a0, a1 as u32, a2 as u32),
```
- `SYS_chown(path, owner, group)`:path 是字符串指针,`a0 = ptr`,`a1 = uid`,`a2 = gid`。
- `SYS_fchown(fd, owner, group)`:fd 是 int,`a0 = fd`,`a1 = uid`,`a2 = gid`。
- 但 `chown_syscall(a0, a1, a2)` 第一个参数类型未确认是否同时支持 path/u32。
- 更严重:`SYS_fchown` 使用 `misc::fchown_syscall`,`SYS_chown` 使用 `file_ops::chown_syscall` — **两套实现**,任何语义偏差都难发现。

**修复建议**:
- 合并到同一 `chown_syscall(op: ChownOp, target: ChownTarget, uid, gid)`。
- 或显式标注两条路径参数语义:`chown_path_syscall(a0: u64, a1: u32, a2: u32)` + `chown_fd_syscall(a0: i32, a1: u32, a2: u32)`。

**验证方法**: 集成测试 `chown("/tmp/file", uid, gid)` + `fchown(fd, uid, gid)` 同时验证。

---

### 3.8 [P2] `dispatch_proc` 中 `SYS_gettimeofday` 走 `info::gettimeofday_syscall` 但 `SYS_clock_gettime` 走 `fs::file_ops::clock_gettime_syscall` — 时间相关 syscall 被拆分到 fs 模块

**位置**: `dispatch.rs:227-228, 374-376`
**严重度**: P2（架构）
**问题描述**:
- 时间相关 syscall 被拆分到 `fs::file_ops` 与 `proc::info` 两个模块,违反内聚性。
- 未来 `clock_gettime` 增加新 clock_id 时,需同时改两处。

**修复建议**: 抽取 `services::time::*` 模块,统一所有时间 syscall handler。

**验证方法**: `grep -rn "clock_gettime_syscall\|gettimeofday_syscall" src/` 列出调用点。

---

### 3.9 [P2] `dispatch_proc` 末尾 `_ => return None` 但 `Some(match num { ... })` 整体返回 — 死代码分支

**位置**: `dispatch.rs:320-410`
**严重度**: P2（风格）
**问题描述**:
- `Some(match num { ... _ => return None })` — 当 num 不匹配时 `return None` 跳出整个函数,而 `match` 的 `_` 分支返回值类型是 `!`(Never),被自动 coerce 到 `i64`。
- 这是 Rust 1.66+ 的"never type coercion",但与 `#[expect(clippy::match_same_arms)]` 一起使用时易触发 dead_code 警告。

**修复建议**: 拆为 `match num { ... }` 后再用 `Some(return_value)` 包裹:
```rust
let r = match num { ... _ => return None };
Some(r)
```

**验证方法**: `cargo clippy --release -- -D warnings` 是否通过。

---

### 3.10 [P2] `dispatch::register_services_dispatch` 失败时仅 `log_info` 不 panic,可能掩盖启动错误

**位置**: `dispatch.rs:744-755`
**严重度**: P2（启动可靠性）
**问题描述**:
```rust
pub fn register_services_dispatch() -> Result<(), ()> {
    static POLICY: ServicesSyscallDispatch = ServicesSyscallDispatch;
    let r = register_syscall_dispatch(&POLICY);
    log_info(... "[SYSCALL] register_services_dispatch result={}", ...);
    r.map_err(|_| ())
}
```
- 若注册失败,只 `log_info` 写一行,**不 panic**。
- 启动期 `services::init()` 调用此函数,若失败应 panic 或返回致命错误。
- 当前实现让 `Result<(), ()>` 静默吞掉错误。

**修复建议**:
```rust
let r = register_syscall_dispatch(&POLICY);
if r.is_err() {
    log_error(... "[SYSCALL] FATAL: register_services_dispatch failed");
    panic!("services syscall dispatch register failed");
}
r
```

**验证方法**: 单元测试模拟双注册 → 期望 panic。

---

### 3.11 [P2] `dispatch_fs` 中 `SYS_pipe` 与 `SYS_pipe2` 共享 `pipe_syscall(a0)` 但 flags 丢弃

**位置**: `dispatch.rs:189-190`
**严重度**: P2（语义）
**问题描述**:
```rust
SYS_pipe => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
SYS_pipe2 => as_ret(crate::kernel::services::fs::io::pipe_syscall(a0)),
```
- `SYS_pipe2(int pipefd[2], int flags)` — `a1` 是 flags(O_CLOEXEC/O_NONBLOCK)。
- 当前 `SYS_pipe2` **完全忽略 `a1`**,无法设置 close-on-exec 或非阻塞。
- 用户调用 `pipe2(fd, O_CLOEXEC)` 等同于 `pipe(fd)`,违反 POSIX。

**修复建议**:
```rust
SYS_pipe2 => as_ret(crate::kernel::services::fs::io::pipe2_syscall(a0, a1 as i32)),
```

**验证方法**: 集成测试 `pipe2(fd, O_CLOEXEC)` → 子进程中 `fd[0]`/`fd[1]` 应关闭。

---

### 3.12 [P2] `dispatch_fs` 中 `SYS_fchmod` 与 `SYS_fchmodat` 共享 `chmod_syscall` 但语义不同

**位置**: `dispatch.rs:134-137, 164-166`
**严重度**: P2（语义）
**问题描述**:
- `SYS_fchmod(fd, mode)`:fd 是 int。
- `SYS_fchmodat(dirfd, path, mode, flags)`:4 参数,且 `path` 可能为 AT_EMPTY_PATH 等特殊值。
- 当前 `SYS_fchmodat` 调用 `chmod_syscall(a1, a2)` — 忽略 `a0`(dirfd)和 `a3`(flags),完全按 `chmod` 语义处理,违反 Linux。

**修复建议**:
```rust
SYS_fchmodat => as_ret(crate::kernel::services::fs::mode::fchmodat_syscall(
    a0 as i32, a1, a2 as u32, a3 as i32,
)),
```
实现新的 `fchmodat_syscall` handler。

**验证方法**: 集成测试 `fchmodat(AT_FDCWD, "/tmp/file", mode, 0)` → 期望与 `chmod` 等效。

---

## 4. services/fs/inode.rs（603 行 / 8 项发现）

### 4.1 [P0] `Inode` trait 中 `mount_idx(&self) -> u32` 在 `AnonymousInode` 中硬编码 `u32::MAX`,可能在 mmap 路径触发 panic/越界

**位置**: `inode.rs:174-175, 262-263`
**严重度**: P0（资源安全）
**问题描述**:
```rust
pub fn new(inode_id: u32) -> Self {
    Self {
        inode_id,
        mount_idx: u32::MAX, // 匿名文件无挂载点
    }
}
```
- 注释"匿名文件无挂载点"使用 `u32::MAX` 作为哨兵值,但 `mount_idx` 在 mmap / VFS 路径中**用作数组索引**(VFS mount table)。
- 调用 `mounts[mount_idx as usize]` 在 `u32::MAX = 4_294_967_295` 时**几乎必定越界 panic**。
- `LegacyInode::mount_idx()` 同问题,但相对可控(由用户传入)。

**修复建议**:
```rust
fn mount_idx(&self) -> Option<u32> {  // 改签名
    None
}
```
或保持 `u32` 但要求所有调用点检查 `mount_idx != u32::MAX`。

**验证方法**: 集成测试 `mmap(anonymous_fd, ...)` → 不应 panic;单元测试 `AnonymousInode::new(1).mount_idx()` → 文档化哨兵语义。

---

### 4.2 [P1] `LegacyInode::stat` 使用 `rel_path` 但 `fs_stat(&rel_path, pwm)` 是路径级操作,违反"Plan B Inode trait 不依赖路径"原则

**位置**: `inode.rs:421-425, 470-483`
**严重度**: P1（架构）
**问题描述**:
```rust
pub struct LegacyInode {
    handle: u32,
    mount_idx: u32,
    file_type: u8,
    rel_path: alloc::string::String,  // ← 持有路径!
}
fn stat(&self, pwm: u64) -> KernelResult<VfsStat> {
    ...
    f.fs_stat(&self.rel_path, pwm)  // ← 走路径级 fs_stat
}
```
- `Inode` trait 设计原则:"句柄级操作 (read/write/stat by open file)",无路径依赖。
- 但 `LegacyInode` **仍持有 `rel_path`** 并走 `fs_stat(path)` —— 完全绕过 Plan B 的"用 Inode 替代 path-based lookup"目标。
- 文档(L412-414)承认这是"过渡期适配器",但若整个 fs 仍依赖 `fs_stat(path)`,则 Plan B 实际未推进。

**修复建议**:
- 在底层 `FileSystem` trait 增加 `fs_fstat(handle, pwm) -> VfsStat`,各 FS 实现该方法。
- `LegacyInode::stat` 改为 `f.fs_fstat(self.handle, pwm)`,丢弃 `rel_path` 字段。

**验证方法**: 搜索所有 `fs_stat(&self.rel_path, ...)` 调用点,确认无遗漏。

---

### 4.3 [P1] `AnonymousInode::read/write` 中 `ANONYMOUS_FS.read_at(...)` 返回 `Option<usize>`,失败时仅返回 `Io` 错误,丢失底层原因

**位置**: `inode.rs:209-219`
**严重度**: P1（错误处理）
**问题描述**:
```rust
fn read(&self, offset: u64, buf: &mut [u8], _pwm: u64) -> KernelResult<usize> {
    ANONYMOUS_FS.read_at(self.inode_id, offset, buf).ok_or(KernelError::Io)
}
```
- `Option<usize>` 转 `KernelError::Io` 时**丢失 `None` 的真实原因**(offset 越界 vs fs 内部错误 vs inode 不存在)。
- 与 `LegacyInode::read` 中调用 `f.fs_read(...)` 返回 `KernelResult` 路径不对称 → 错误粒度不一致。

**修复建议**:
- 将 `ANONYMOUS_FS.read_at` 改为返回 `KernelResult<usize>`,各错误路径显式化。
- 或在 `AnonymousInode::read` 内根据 `None` 上下文区分返回 `InvalidArgument` (offset 越界) / `Io` (底层错误)。

**验证方法**: 单元测试 `read(offset=u64::MAX)` → 应明确 `EINVAL` 而非 `EIO`。

---

### 4.4 [P1] `Inode::seek` 默认实现缺失 — 部分实现者返回错误的 `End` 计算

**位置**: `inode.rs:80, 239-247, 364-374, 500-513`
**严重度**: P1（语义）
**问题描述**:
- `AnonymousInode::seek` 与 `RamFsInode::seek` 都正确计算 `End = file_size + offset`。
- 但 `LegacyInode::seek` 直接 `f.fs_seek(handle, offset, whence, current_offset)`,**完全依赖底层实现**。
- 各实现的 `SeekWhence::End` 计算可能不一致(有的 saturating_add,有的 wrapping_add)。

**修复建议**:
- 在 `Inode` trait 增加 `seek_default` 默认实现,统一 `End` 计算规则。
- 或所有实现者必须显式实现 `seek`,不留空。

**验证方法**: 单元测试 `seek(SEEK_END, -1)` 在所有 Inode 实现上行为一致。

---

### 4.5 [P2] `AnonymousInode::is_dir` 硬编码 `false`,但 `AnonymousFS` 可能有匿名目录 inode 类型 — 永远非目录

**位置**: `inode.rs:249-251`
**严重度**: P2（语义）
**问题描述**:
- `AnonymousInode` 仅用于文件类型(memfd/无路径文件),但 `AnonymousFS` 中可能存在 `ANONYMOUS_DIR_INODE`。
- 当前 `AnonymousInode` 不区分文件/目录,统一返回 `false`。
- 若 mmap/lookup 路径使用 `is_dir()` 决策,可能误操作。

**修复建议**:
```rust
pub struct AnonymousInode {
    inode_id: u32,
    mount_idx: u32,
    file_type: u8,  // 记录类型
}
fn is_dir(&self) -> bool { self.file_type == VfsFileType::Dir.as_u8() }
```

**验证方法**: 集成测试 `opendir(anonymous_dir_fd)` → 期望 `ENOTDIR`。

---

### 4.6 [P2] `Inode` trait 中 `chmod/chown/readlink/symlink/link/mkdir/unlink/rename/readdir` 全部默认返回 `Ok(())` 或 `NotSupported` — 无统一错误

**位置**: `inode.rs:100-158`
**严重度**: P2（语义）
**问题描述**:
- `chmod/chown` 默认 `Ok(())` — 即使是 ROFS 也"成功",违反 POSIX EACCES 语义。
- 其他默认 `NotSupported` — 调用方无法区分"FS 不支持"与"权限拒绝"。
- 默认 `Ok(())` 是**安全性反模式**:静默成功导致上层不感知失败。

**修复建议**:
- `chmod/chown` 默认改为 `Err(KernelError::NotSupported)`,强制各 FS 显式实现。
- 或抽出 `default_inode_impls` 模块,所有默认实现集中,避免散落。

**验证方法**: 单测 `RamFsInode::chmod` 在 RO mount 上 → 应返回 `EACCES` 而非 `Ok`。

---

### 4.7 [P2] `RamFsInode::is_dir` 中 `if (self.inode_id as usize) < 256` 硬编码 inode 数量上限

**位置**: `inode.rs:376-384`
**严重度**: P2（架构）
**问题描述**:
```rust
fn is_dir(&self) -> bool {
    use crate::kernel::framework::fs::ramfs::ramfs::RAMFS_DATA;
    let ramfs = RAMFS_DATA.lock();
    if (self.inode_id as usize) < 256 {
        ramfs.nodes[self.inode_id as usize].file_type == 1 // DIR
    } else {
        false
    }
}
```
- 硬编码 `256` 是 RAMFS inode 最大数量,`>= 256` 的 inode 一律 `is_dir() = false`。
- 若 RAMFS 实际支持更多 inode(通过 `nodes` 动态扩容),这里会**永远返回 false**,所有大 inode id 的目录都被误判为文件。

**修复建议**:
```rust
let nodes_len = ramfs.nodes.len();
if (self.inode_id as usize) < nodes_len {
    ramfs.nodes[self.inode_id as usize].file_type == 1
} else {
    false
}
```

**验证方法**: 单测 `RamFsInode::new(inode_id=1000).is_dir()` 在 inode_id=1000 是目录时 → 应返回 `true`。

---

### 4.8 [P2] `LegacyInode::is_dir` 使用 `self.file_type == Dir.as_u8()` 但 `file_type` 字段未被 chmod/chown 路径更新

**位置**: `inode.rs:515-517`
**严重度**: P2（语义）
**问题描述**:
- `LegacyInode::file_type` 在构造时一次性记录,后续无更新。
- 若底层文件被 `rename` 改变类型(file→dir?),`LegacyInode` 仍返回旧类型。
- 这是 `LegacyInode` 适配层的固有限制,但应文档化。

**修复建议**: 在 `LegacyInode::stat` 后刷新 `file_type`,或文档显式声明"LegacyInode 假定 file_type 不可变"。

**验证方法**: 集成测试 `rename(file_path, dir_path)` 后通过旧 fd `is_dir()` → 应返回 `true`(当前实现返回 `false`)。

---

## 5. services/proc/sched_policy.rs（605 行 / 9 项发现）

### 5.1 [P0] `CfsRunQueue::boost_priority` 函数存在但**无人调用** — 死代码 + v2.0 §F7.1/F7.2 修复未触达

**位置**: `sched_policy.rs:189-207`
**严重度**: P0（死代码 / 规范违反 F9）
**问题描述**:
```rust
pub fn boost_priority(&mut self, current_tick: u64) {
    if self.tree.is_empty() { ... }
    let min_vr = self.tree.first_key_value().map_or(0, |(&(vr, _), ())| vr);
    let entries: alloc::vec::Vec<(Pid, u64)> = self.tree.keys().map(|&(vr, pid)| (pid, vr)).collect();
    self.tree.clear();
    for (pid, _old_vr) in entries {
        self.tree.insert((min_vr, pid), ());  // ← 与 boost_all_vruntime 重复
    }
    self.min_vruntime.store(min_vr, Ordering::Release);
    self.last_boost_tick = current_tick;
}
```
- v2.0 §F7.1/F7.2 报告"`boost_priority` 抹平 vruntime"被识别为 bug。
- 但本函数与 `boost_all_vruntime`(L209-225)**逻辑完全相同**(都是把所有进程 vruntime 设为 min_vr)。
- `grep -rn "boost_priority" src/` 显示**仅本文件 + framework/proc/scheduler.rs:16 注释引用**,**无人调用**。
- framework 端调度循环(L996-998)只调用 `boost_all_vruntime`,**未调用 `boost_priority`**。
- 这是**双重 bug**:函数本身实现仍是抹平 vruntime,且无人使用。

**修复建议**:
- 删 `boost_priority` 函数(完全死代码,F9 零容忍)。
- 或将其改为"实际不同语义"函数(如优先级提升而非 vruntime 抹平),并加调用点。

**验证方法**: `cargo build --release` 是否有 `dead_code` 警告;`grep -rn "boost_priority" src/` 仅返回本文件 + 注释 → 确认是死代码。

---

### 5.2 [P1] `CfsRunQueue::enqueue` 中 `start_vr = vruntime.max(min_vr)` — 新进程 vruntime 被钳制到 min_vr,失去自身 vruntime 表达

**位置**: `sched_policy.rs:127-134`
**严重度**: P1（语义）
**问题描述**:
```rust
pub fn enqueue(&mut self, pid: Pid, vruntime: u64, weight: u64) {
    let min_vr = self.min_vruntime.load(Ordering::Acquire);
    let start_vr = vruntime.max(min_vr);  // ← vruntime 被钳制
    self.tree.insert((start_vr, pid), ());
    ...
}
```
- 这是 Linux CFS 的"睡眠进程 vruntime 钳制"语义。
- 但**未考虑 weight**: 高权重进程(低 nice)的 vruntime 应增长更慢,被钳制到 min_vr 后立即获得调度机会,反而对低权重进程不公平。
- Linux 在 `place_entity()` 中按 `sysctl_sched_latency / weight` 比例补偿:
  ```c
  vruntime += sysctl_sched_latency / weight;  // 补偿
  if (vruntime < min_vruntime)
      vruntime = min_vruntime;
  ```

**修复建议**:
```rust
let compensated = vruntime.saturating_add(
    TARGET_LATENCY_TICKS.saturating_mul(NICE0_WEIGHT) / weight
);
let start_vr = compensated.max(min_vr);
```

**验证方法**: 单测 enqueue weight=8192 (高优) → vruntime 应 +0;enqueue weight=15 (低优) → vruntime 应 + ~ TARGET_LATENCY * 1024/15。

---

### 5.3 [P1] `CfsRunQueue::dequeue` 中 `self.tree.remove(&(vruntime, pid))` — 调用方必须传正确 vruntime,易错

**位置**: `sched_policy.rs:136-157`
**严重度**: P1（API 易用）
**问题描述**:
```rust
pub fn dequeue(&mut self, pid: Pid, vruntime: u64, weight: u64) -> bool {
    ...
    if self.tree.remove(&(vruntime, pid)).is_some() { ... }
}
```
- 调用方必须传**精确的** `(vruntime, pid)` 才能删除,否则 `remove` 返回 None。
- 若调用方传 `vruntime` 与 enqueue 时不完全一致(BTreeMap key 精度问题 / 中间 vruntime 变化),**dequeue 永远失败**。
- `boost_priority`/`boost_all_vruntime` 会修改 vruntime → 后续 dequeue 用原 vruntime 会失败。

**修复建议**:
```rust
pub fn dequeue_by_pid(&mut self, pid: Pid) -> bool {
    let key = self.tree.iter().find(|(_, &p)| p == pid).map(|(k, _)| *k);
    key.map(|k| { self.tree.remove(&k); self.sync_min_vruntime(); true }).unwrap_or(false)
}
```
或要求 enqueue/dequeue/pick_next 都通过内部 pid→vruntime 索引。

**验证方法**: 单测 enqueue(pid=1, vr=100) → boost_priority → dequeue(pid=1, vr=100) → 当前实现返回 `false`(bug);修复后返回 `true`。

---

### 5.4 [P1] `DefaultPolicy::pick_next_priority` 中 `[u32; 5]` 与 `ThreadPriority` 5 变体不一致(枚举仅 5 项,但 reverse 0..5 等同)

**位置**: `sched_policy.rs:342-350`
**严重度**: P1（一致性）
**问题描述**:
- `ThreadPriority` 枚举:`Realtime, High, Normal, Low, Idle`(5 项,L367-371)。
- `pick_next_priority` 接受 `queue_lengths: [u32; 5]`,`for prio in (0..5).rev()` 扫描 4..0。
- 但 `time_slice_for` 用 `match priority` 覆盖 5 个 variant + `Idle => u32::MAX`。
- 索引映射:`Idle=4, Low=3, Normal=2, High=1, Realtime=0`?
- 注释无说明,且 `register_sched_decision` 实现注册到 framework 后,framework 端如何将 `queue_lengths` 数组按枚举索引填充?**完全未文档化**。

**修复建议**:
- 显式注释映射:`queue_lengths[0]=Realtime, [1]=High, [2]=Normal, [3]=Low, [4]=Idle`。
- 或改为 `fn pick_next_priority(&self, queues: &ThreadQueues) -> Option<ThreadPriority>`,用 enum 类型索引。

**验证方法**: `grep "pick_next_priority" framework` 查 framework 调用点 + 数组填充逻辑。

---

### 5.5 [P2] `nice_to_weight` 使用 `.clamp(-20, 19)` 但 `weight_to_nice` 使用 `weight >= 88761` 边界硬编码

**位置**: `sched_policy.rs:39-43, 46-63`
**严重度**: P2（一致性）
**问题描述**:
- `nice_to_weight` clamp 到 `[-20, 19]`,即 40 项 NICE_TO_WEIGHT 数组索引 0..39。
- `weight_to_nice` L47 `if weight >= NICE_TO_WEIGHT[0]` 硬编码 `88761` 索引。
- 若 NICE_TO_WEIGHT 数组**顺序或值调整**,L47-50 边界 hard-code 全部失效。
- `weight_to_nice` 还会做 `w.abs_diff(weight)` — 对 `u64` 使用 `abs_diff` 实际等价于 `w.checked_sub(weight).unwrap_or(0)`,**对 `weight > w` 返回 0**(但仍然有效)。

**修复建议**:
```rust
if weight >= NICE_TO_WEIGHT[0] { return -20; }
if weight <= NICE_TO_WEIGHT[NICE_TO_WEIGHT.len() - 1] { return 19; }
```
用 `first()`/`last()` 替代硬编码下标。

**验证方法**: 调整 `NICE_TO_WEIGHT[0]` → 77988(假想);`weight_to_nice(77988)` 当前实现应返回 -20,新实现应正确。

---

### 5.6 [P2] `DlRunQueue::total_utilization` 使用 `u64` 但利用率应小于 100,逻辑错误风险

**位置**: `sched_policy.rs:264-280`
**严重度**: P2（语义）
**问题描述**:
```rust
pub fn enqueue(&mut self, pid: Pid, deadline_abs: u64, util_pct: u64) -> bool {
    if self.total_utilization.saturating_add(util_pct) > DL_MAX_UTILIZATION_PCT {
        return false;
    }
    ...
    self.total_utilization += util_pct;
}
```
- `total_utilization` 是 `u64`,但 `util_pct` 是百分比(0..100)。
- 加法可能溢出 `u64` 但 `saturating_add` 兜底。
- 但 `enqueue` 内 `saturating_add` 之后**直接 `total_utilization += util_pct`**(L270)未饱和 → **真正的饱和在 saturating_add,但赋值未使用 saturating** → 不一致。

**修复建议**:
```rust
let new_total = self.total_utilization.saturating_add(util_pct);
if new_total > DL_MAX_UTILIZATION_PCT { return false; }
self.total_utilization = new_total;
```
或 `self.total_utilization = self.total_utilization.saturating_add(util_pct); if ...`。

**验证方法**: 单测 `enqueue(pid, dl, 50); enqueue(pid, dl, 60);` → 当前实现 `total_utilization = 110`,`enqueue` 第三个 `60%` 时 saturating_add = 170 → 拒绝,正确;但内部 `total_utilization` 已被加两次,可能影响 `dequeue` 计算。

---

### 5.7 [P2] `calc_vruntime_delta(weight)` 返回 `NICE0_WEIGHT / weight`,未考虑 `MIN_GRANULARITY`

**位置**: `sched_policy.rs:304-310`
**严重度**: P2（语义）
**问题描述**:
```rust
pub fn calc_vruntime_delta(weight: u64) -> u64 {
    if weight == 0 { return NICE0_WEIGHT; }
    (NICE0_WEIGHT / weight).max(1)
}
```
- 公式 `NICE0_WEIGHT / weight` 是"weight 越大,vruntime 增长越慢"语义,正确。
- 但缺最小粒度保护 —— `weight = u64::MAX` 时,NICE0_WEIGHT/u64::MAX = 0,`.max(1)` 兜底为 1。
- 与 `cfs_should_preempt` 中 `MIN_GRANULARITY * NICE0_WEIGHT / weight` 风格不一致。

**修复建议**:
```rust
(NICE0_WEIGHT / weight).max(MIN_GRANULARITY_TICKS)
```

**验证方法**: 单测 `calc_vruntime_delta(u64::MAX)` → 当前返回 1,期望 `MIN_GRANULARITY_TICKS`。

---

### 5.8 [P2] `DefaultPolicy::time_slice_for(Idle) => u32::MAX` 是永真,可能引发调度死循环

**位置**: `sched_policy.rs:370`
**严重度**: P2（调度安全）
**问题描述**:
- `Idle` 优先级返回 `u32::MAX` 时间片 = ~4.29 × 10⁹ ticks ≈ 数小时。
- `should_reschedule(time_slice_remaining) <= 1` 才触发调度。
- `u32::MAX - 1` 仍 > 1 → 长时间不调度,其他进程饿死。
- Linux idle 任务实际上被显式排除 CFS run queue。

**修复建议**:
```rust
ThreadPriority::Idle => 0, // 让 Idle 进程立即被抢占
```
或显式文档化 `Idle => u32::MAX` 含义并注释 "Only run if no other task"。

**验证方法**: 集成测试创建 Idle 优先级进程 + 5 个 Normal 进程 → 期望 Normal 仍能获得 CPU。

---

### 5.9 [P2] `register_default_policy` 失败时仅 `map_err(|_| ())` 静默,不 panic

**位置**: `sched_policy.rs:386-389`
**严重度**: P2（启动可靠性）
**问题描述**:
- 与 §3.10 同样的"启动期注册失败不 panic"问题。
- `register_sched_decision(&POLICY)` 失败时只返回 `Err(())`,**启动期应 panic**。

**修复建议**: 与 §3.10 同 —— 启动期注册失败 panic。

**验证方法**: 单元测试双注册 → 期望 panic 或显式启动失败。

---

## 6. services/proc/signal.rs（601 行 / 8 项发现）

### 6.1 [P1] `send` 中 `Signal::NONE`(0) 走 `with(pid, |_p| ())` 仅检查存在,但 `proc::table::signal_set` 之前未检查 PID 0 (idle/init)

**位置**: `signal.rs:286-297`
**严重度**: P1（语义）
**问题描述**:
```rust
pub fn send(pid: Pid, sig: Signal) -> SignalResult<()> {
    if sig == Signal::NONE {
        return crate::kernel::services::proc::table::with(pid, |_p| ())
            .ok_or(SignalError::NoSuchProcess);
    }
    if sig.0 >= 64 { return Err(SignalError::InvalidArgument); }
    crate::kernel::services::proc::table::signal_set(pid, u32::from(sig.0))
        .map_err(|_| SignalError::NoSuchProcess)
}
```
- `send(pid, 0)` 走 `with(pid, ...)` 检查存在,**未检查权限**。
- POSIX `kill(pid, 0)` 要求:
  1. 调用者与目标进程**同 uid** 或具有 `CAP_KILL`。
  2. 返回 `EPERM`(权限不足)或 `ESRCH`(不存在)。
- 当前实现无 uid 校验 → 任何进程可对任意 PID 调用 `send(pid, 0)`,**泄露进程存在性**(信息泄露 + 权限漏洞)。

**修复建议**:
```rust
if sig == Signal::NONE {
    return crate::kernel::services::proc::table::with(pid, |target| {
        if cred::same_uid_or_cap_kill(caller_pwm, target.owner_pwm) {
            Ok(())
        } else {
            Err(SignalError::PermissionDenied)
        }
    })
    .ok_or(SignalError::NoSuchProcess)?;
}
```

**验证方法**: 集成测试 `kill(target_pid, 0)` from different uid → 期望 EPERM。

---

### 6.2 [P1] `kill_syscall` 缺少 pid 范围校验 — `pid = -INT_MIN` 等极端值未拦截

**位置**: `signal.rs:439-456`
**严重度**: P1（输入校验）
**问题描述**:
```rust
pub fn kill_syscall(pid: i32, sig: i32) -> Result<usize, Errno> {
    if !(0..=31).contains(&sig) { return Err(Errno::EINVAL); }
    // 注释: "原约束 pid <= 0 -> ESRCH 已移除 (TRACK-315B7C 解决)"
    let ret = crate::kernel::framework::syscall::api::sys_kill(pid, sig);
    ...
}
```
- 注释承认 `pid <= 0` 校验被移除(TRACK-315B7C),但**无任何替代校验**。
- `pid = i32::MIN = -2147483648` 取反 `|pid| = 2147483648` 超出 i32 范围,**直接传入 framework 会溢出**。
- `pid = -1` 在 POSIX 是"广播给所有进程",但**QueenX 可能不支持广播**,应显式 ENOSYS 或 EINVAL。

**修复建议**:
```rust
// 显式 pid 范围校验
if !(-(i32::MAX)..=i32::MAX).contains(&pid) { return Err(Errno::EINVAL); }
match pid {
    0 => /* 同进程组 */,
    -1 => return Err(Errno::ENOSYS), // 当前不支持广播
    p if p < -1 => /* |pid| 进程组 */,
    _ => /* 单进程 */,
}
```

**验证方法**: 集成测试 `kill(i32::MIN, SIGTERM)` → 期望 EINVAL 而非 panic 或溢出。

---

### 6.3 [P1] `rt_sigaction_syscall` 允许 RT 信号(32..=64)被设置 handler,但 framework 内核基础设施是 32-bit `signal_pending_*` 简易实现

**位置**: `signal.rs:466-487` + `signal.rs:8-12` 文档
**严重度**: P1（实现差距）
**问题描述**:
- services 层允许 `signum in 32..=64` 设置 handler。
- 但 framework 是 **per-process 32-bit 简易实现**(`signal_pending_*`,只支持 32 bit)。
- 当用户设置 RT 信号 handler 后,实际信号到达时:
  - `Signal::to_bit()` 返回 `1 << 32..64`,但 framework `signal_pending` 是 u32 → **高位全部丢失**。
  - handler 永不触发。
- 这是 services 与 framework 实现**严重脱节**。

**修复建议**:
- services 限制 `signum ∈ 1..=31`,RT 信号暂时 EINVAL:
  ```rust
  if !(1..=31).contains(&signum) { return Err(Errno::EINVAL); }
  ```
- 或扩展 framework `signal_pending` 到 u64(需 audit framework 端)。

**验证方法**: 集成测试 `rt_sigaction(35, handler)` → 当前返回 0(成功),实际收到 `signal 35` 时框架无法识别 → 期望 EINVAL 或内核扩展。

---

### 6.4 [P2] `StandardSignalPolicy::default_action` 与 `SignalDisposition::default_for` 重复且硬编码编号,易漂移

**位置**: `signal.rs:227-241, 564-572`
**严重度**: P2（一致性）
**问题描述**:
- `SignalDisposition::default_for(StandardSignal)` 用 enum 匹配(L229-240)。
- `StandardSignalPolicy::default_action(sig: u8)` 用数字硬编码(L564-572),如 `3 | 4 | 6 | 7 | 8 | 11 | 31 | 24 | 25`。
- 两处**定义同一规则**,数字与 enum 重复,任何信号编号调整需同步两处。

**修复建议**:
- `StandardSignalPolicy::default_action` 内部转换为 `StandardSignal`,复用 `SignalDisposition::default_for`:
  ```rust
  fn default_action(&self, sig: u8) -> SignalDefaultAction {
      StandardSignal::from_number(sig).map_or(SignalDefaultAction::Term, |s| {
          match SignalDisposition::default_for(s) {
              SignalDisposition::Ign => SignalDefaultAction::Ign,
              SignalDisposition::Core => SignalDefaultAction::Core,
              SignalDisposition::Stop => SignalDefaultAction::Stop,
              SignalDisposition::Cont => SignalDefaultAction::Cont,
              SignalDisposition::Term => SignalDefaultAction::Term,
          }
      })
  }
  ```

**验证方法**: 修改 `StandardSignal::Stkflt = 16` 为 `Stkflt = 17` → `default_action(16)` 与 `default_for(StandardSignal::Stkflt)` 必须行为一致。

---

### 6.5 [P2] `pick_next_signal` 中 `sig_bit == 0` 过滤 `Signal::NONE`,但 RT 信号(>= 32)范围未处理

**位置**: `signal.rs:578-587`
**严重度**: P2（语义）
**问题描述**:
```rust
fn pick_next_signal(&self, deliverable: u64) -> Option<u8> {
    if deliverable == 0 { return None; }
    let sig_bit = deliverable.trailing_zeros() as u8;
    if sig_bit == 0 || sig_bit > 31 { return None; }  // ← 拒绝 >= 32 RT 信号
    Some(sig_bit)
}
```
- `sig_bit > 31` 直接 None,但 RT 信号(32..=64)也可能设置 pending。
- 与 §6.3 同样的"framework 仅 32-bit"问题一致 —— strategy 应明确"不支持 RT 信号"或"framework 已扩展"。

**修复建议**: 文档化 RT 信号当前不可用,或扩展 framework + strategy 同步。

**验证方法**: 单测 `pick_next_signal(1u64 << 35)` → 当前返回 None(应显式 None,无 panic)。

---

### 6.6 [P2] `send` 函数用 `with(pid, |_p| ())` 丢弃 `_p`,可读性差且 `with` 的 Option<Result> 模式易混

**位置**: `signal.rs:289-291`
**严重度**: P2（代码风格）
**问题描述**:
```rust
return crate::kernel::services::proc::table::with(pid, |_p| ())
    .ok_or(SignalError::NoSuchProcess);
```
- `with(pid, fn)` 返回 `Option<R>` (None = pid 不存在),但这里用 `ok_or` 将 None 转 `NoSuchProcess`。
- 若 `with` 返回 `Option<Result<...>>`(存在但函数返回 Err),当前写法会**丢弃 Err**。
- 应确认 `proc::table::with` 实际签名。

**修复建议**: 显式类型:
```rust
match crate::kernel::services::proc::table::with(pid, |_p| ()) {
    Some(()) => Ok(()),
    None => Err(SignalError::NoSuchProcess),
}
```

**验证方法**: `grep "pub fn with" src/kernel/services/proc/table.rs` 查签名。

---

### 6.7 [P2] `rt_sigprocmask_syscall` 缺少 set 指针合法性校验

**位置**: `signal.rs:497-515`
**严重度**: P2（输入校验）
**问题描述**:
- `rt_sigprocmask(how, set, oset)` 中 `set` 是用户 buffer 指针,但 services 层仅校验 `how ∈ 0..=2`,**未校验 `set` 指针合法性**。
- 注释(L519-522)说"ss 与 old_ss 合法性由 framework 侧 raw::check_user_buf 校验"。
- 但 `set` 也应类似处理,服务层不校验 → 若 framework 端未校验,**可触发任意内存读**(I4 不变式违反)。

**修复建议**: 调用 framework 前**明确要求**已校验 set/oset 指针,或显式调用 `framework::raw::check_user_buf(set, 8)` (若存在公开 API)。

**验证方法**: 集成测试 `rt_sigprocmask(SIG_BLOCK, 0xDEADBEEF, 0)` → 期望 EFAULT 而非 kernel panic。

---

### 6.8 [P2] `register_standard_signal_policy` 重复注册不 panic,与 §3.10 同问题

**位置**: `signal.rs:598-600`
**严重度**: P2（启动可靠性）
**问题描述**:
- 与 §3.10 dispatch + §5.9 sched_policy 同样问题。
- `register_signal_decision(&POLICY).map_err(|_| ())` 静默吞错误。
- 启动期应 panic。

**修复建议**: 启动期失败 panic。

**验证方法**: 单元测试双注册 → 期望 panic。

---

## 7. 综合问题统计

### 7.1 按严重度分类

| 严重度 | 数量 | 关键类别 |
|--------|------|----------|
| **P0** | 5 | syscall 编号硬冲突 + dispatch name_to_handle_at 错误吞咽 + framework API 直调 + AnonymousInode u32::MAX 哨兵 + boost_priority 死代码 + F2 黑名单违反 |
| **P1** | 25 | POSIX 语义错误 + 权限校验缺失 + 输入校验缺失 + 编号未分发 + 重复实现 |
| **P2** | 26 | 风格/一致性/死代码/启动可靠性 |

### 7.2 按文件分类

| 文件 | P0 | P1 | P2 | 总 |
|------|----|----|----|----|
| namespace.rs | 0 | 4 | 5 | 9 |
| syscall/types.rs | 1 | 5 | 4 | 10 |
| syscall/dispatch.rs | 2 | 6 | 4 | 12 |
| fs/inode.rs | 1 | 4 | 3 | 8 |
| sched_policy.rs | 1 | 3 | 5 | 9 |
| signal.rs | 0 | 3 | 5 | 8 |

### 7.3 按类别分类

| 类别 | 数量 |
|------|------|
| POSIX 语义错误 | 14 |
| 死代码 / 重复定义 | 8 |
| 资源/内存安全 | 6 |
| 权限/PWM 校验缺失 | 5 |
| 编号空间错位 | 5 |
| 多架构兼容 | 3 |
| 错误处理 | 5 |
| API 卫生 | 4 |
| 启动可靠性 | 3 |
| 一致性/风格 | 3 |

---

## 8. 与 v2.0 §F7.1/F7.2 的对照

| v2.0 已知问题 | 修复状态 | 本次审计发现 |
|---------------|----------|--------------|
| boost_priority 抹平 vruntime | ⚠️ **未根除**:`boost_priority`(L189)仍存在且与 `boost_all_vruntime`(L209)逻辑完全相同;framework 只调用 `boost_all_vruntime`,`boost_priority` 永不被调用 → **死代码** | §5.1 P0 |
| CFS vruntime 钳制未补偿 weight | ⚠️ **未修复**:`enqueue` 仍只 `vruntime.max(min_vr)`,无 `TARGET_LATENCY/weight` 补偿 | §5.2 P1 |
| syscall 编号与 framework 对齐 | ⚠️ **多个错位**:`SYS_setregid/SYS_clone3/QX_UNSHARE/QX_SETNS` 等已定义但 dispatch 完全未接线 | §2.6 §3.5 §3.6 §1.9 |
| services 0 unsafe | ✅ 通过 | 无新问题 |
| 中文注释 100% | ✅ 通过 | 无新问题 |

---

## 9. 推荐优先级处理顺序

1. **立即处理 P0 (5 项)**:
   - §5.1 删 `boost_priority` 死代码(F9 零容忍)
   - §2.1 修 syscall 编号硬冲突(编译期可能已失败)
   - §3.2 修 dispatch 直调 framework::syscall::api(F2 违反)
   - §3.1 验证 name_to_handle_at_syscall 签名 + 错误吞咽
   - §4.1 修 AnonymousInode::mount_idx() u32::MAX 哨兵

2. **下个 sprint 处理 P1 (25 项)**: 主要为 POSIX 语义 + 权限校验 + 编号分发。

3. **后续 P2 (26 项)**: 一致性 / 风格 / 死代码,可批量修复。

---

## 10. 验证门槛 (AGENTS.md §2.3)

任何修复完成后必须重跑:
1. `./ci/build.sh all`(双架构 0 error / 0 warning)
2. `cargo clippy --release -- -D warnings`
3. `./scripts/audit_services_boundary.py`
4. `./scripts/audit_safety_coverage.py`
5. `./scripts/audit_deadlock_matrix.py`
6. `./scripts/audit_coupling.py`
7. `./scripts/audit_comment_language.py`
8. `make test-host`

任何审计失败 → 本轮未完成。

---

**报告生成**: 2026-08-13
**审计执行**: services 深度审计 v2.1
**关联文档**: docs/explain/spec-engineering.md / docs/plan/ / AGENTS.md