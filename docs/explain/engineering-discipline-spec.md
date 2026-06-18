# 工程纪律性规范

> 约束**后续新代码**的工程规范. 已有代码的解耦进度见 `docs/plan/engineering-discipline.md`.
> 所有开发者 (含 AI) 提交新代码前必须遵守本文件.

---

## 0. 铁律 (零容忍, 违反即拒收)

| # | 规则 | 检查方式 |
|---|------|----------|
| F1 | services 层 **0 unsafe** | `#![deny(unsafe_code)]` 编译期 + `audit_services_boundary.py` |
| F2 | services 禁止访问 framework 内部模块 | `audit_services_boundary.py` 黑名单 |
| F3 | 新增代码禁止引入模块间循环依赖 | `audit_coupling.py` |
| F4 | framework 任何 unsafe 块必须配 `// SAFETY:` 注释 | `audit_safety_coverage.py` |
| F5 | 双架构编译 0 warning 0 error | `make ARCH=x86_64 && make ARCH=aarch64` |
| F6 | 三审计全部通过 | `audit_services_boundary.py` + `audit_safety_coverage.py` + `audit_deadlock_matrix.py` |

---

## 1. 禁止耦合

### 1.1 跨子系统禁止直接访问内部

新代码中, 子系统 A 调用子系统 B 时, **只能通过 B 的 `mod.rs` re-export 或 `api.rs`**, 不得直接访问 B 的内部子模块.

```rust
// ✗ 禁止 — 直接访问内部
use crate::kernel::framework::mm::pmm;
use crate::kernel::framework::sync::raw;
use crate::kernel::framework::arch::x86_64;
use crate::kernel::framework::proc::scheduler_ex;

// ✓ 正确 — 通过顶层 re-export
use crate::kernel::framework::mm;
use crate::kernel::framework::sync;
use crate::kernel::framework::arch;
use crate::kernel::framework::proc;
```

### 1.2 禁止隐式依赖传递

A 依赖 B, B 依赖 C, **A 不得直接使用 C 的类型/函数** (除非 A 也显式声明依赖 C).

```rust
// ✗ 禁止 — fs 直接用 syscall 的 Errno, 形成 fs→syscall 隐式依赖
use crate::kernel::framework::syscall::Errno;

// ✓ 正确 — 使用 framework 统一 Errno 入口
use crate::kernel::framework::errno::Errno;
```

### 1.3 禁止 services 层反向依赖

services A → services B → framework → services A 形成循环时, 必须通过 trait 注入或回调接口解耦.

### 1.4 禁止跨层传递内部类型

framework 内部类型 (如 `Process`, `MmStruct` 的裸字段) 不得作为 services 公开 API 的参数或返回值.

```rust
// ✗ 禁止 — 暴露 framework 内部类型
pub fn do_something(proc: &framework::proc::Process) { ... }

// ✓ 正确 — 使用 PID 或句柄
pub fn do_something(pid: Pid) { ... }
```

---

## 2. 禁止硬编码

### 2.1 跨子系统常量

子系统 A 中使用的、属于子系统 B 的常量, 必须在 `framework::config` 或 `services::config` 中定义, 不得在 A 内部硬编码.

```rust
// ✗ 禁止 — 在 mm 中硬编码 syscall 常量
const MAX_FD: usize = 256;  // 这是 proc 的常量

// ✓ 正确 — 从 config 导入
use crate::kernel::framework::config::MAX_FD;
```

### 2.2 魔数

所有数字常量必须有命名. 唯一例外: `0`, `1`, `-1` 等上下文明确的字面量.

```rust
// ✗ 禁止
if flags & 0x10 != 0 { ... }

// ✓ 正确
const MAP_SHARED: u32 = 0x10;
if flags & MAP_SHARED != 0 { ... }
```

### 2.3 字符串路径

文件系统路径、设备名等不得在代码中硬编码. 通过配置或参数传入.

---

## 3. 禁止耦合性代码

### 3.1 禁止 "顺手优化"

修改现有代码时, **只改必须改的内容**. 不得:
- 顺手重命名无关变量/函数
- 顺手删除"看似无用"的代码
- 顺手调整不相关代码的格式/顺序
- 顺手添加未被要求的错误处理

每一行改动必须能追溯到用户请求或明确的 bug 修复.

### 3.2 禁止过度抽象

- 不为单次使用的代码创建 trait/抽象
- 不为"将来可能需要"预留扩展点
- 三行重复代码优于一个过早抽象
- 问自己: "一个资深工程师会认为这太复杂了吗?" 如果是, 简化

### 3.3 禁止功能膨胀

新功能的实现范围必须严格匹配需求. 不得:
- 添加需求未提及的 "辅助功能"
- 添加未被要求的 "灵活性" 或 "可配置性"
- 为不可能发生的场景写错误处理

---

## 4. 代码质量

### 4.1 services 层新文件

每个新 `.rs` 文件首行:

```rust
#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
```

### 4.2 framework 层新 unsafe

每个 `unsafe` 块必须配:

```rust
// SAFETY: <前提条件>; <调用方保证>; <硬件契约>
unsafe { ... }
```

### 4.3 锁规范

| 上下文 | 允许 | 禁止 |
|--------|------|------|
| 普通内核代码 | `Mutex`, `RwLock`, `SpinLock` | — |
| 中断上下文 | `IrqSpinLock` + disable IRQ | `Mutex`, `RwLock` (会 sleep) |
| 中断处理函数 | 仅自旋锁 + disable IRQ | 任何 sleep 锁 |

### 4.4 错误处理

- services 层返回 `Result<T, Errno>`
- **禁止** 在 services 中无条件 `unwrap()` / `expect()`
- framework 内部失败必须转换为 `Errno` 再返回用户态

### 4.5 日志

- services: `slog_info!` / `slog_warn!` / `slog_err!`
- 禁止: `println!` (no_std 不可用), `klog_ffi!` (unsafe)

---

## 5. 中断上下文纪律

中断上下文 (ISR / softirq / timer callback) 是内核中最危险的执行环境. 一次不当操作可导致不可复现的死锁或数据损坏.

### 5.1 中断上下文禁止操作

| 禁止操作 | 原因 | 后果 |
|----------|------|------|
| 持有 `Mutex` / `RwLock` | 这些锁可能 sleep, 中断上下文不可调度 | 死锁 |
| 调用 `kmalloc(GFP_KERNEL)` | 可能触发页面回收, 需要调度 | 死锁 |
| 调用 `schedule()` / `yield()` | 中断上下文不可让出 CPU | 未定义行为 |
| 调用可能阻塞的 I/O 操作 | 块设备 I/O 可能 sleep | 死锁 |
| 递归获取同一自旋锁 | 即使已 disable IRQ, 递归仍会死锁 | 死锁 |

### 5.2 中断上下文允许操作

- `IrqSpinLock` (已 disable IRQ 的自旋锁)
- 原子操作 (`AtomicU32`, `AtomicU64`, `AtomicBool`)
- 读取全局只读状态
- 写入 per-CPU 变量
- 调用 `klog` 输出 (串口是内存映射 I/O, 不阻塞)

### 5.3 中断下半部设计

需要在中断中做复杂处理时, 必须设计下半部 (bottom half):

```rust
// ✗ 禁止 — 在 ISR 中做完整处理
fn timer_handler() {
    let data = process_data();  // 可能耗时
    update_statistics(data);    // 可能需要锁
}

// ✓ 正确 — ISR 仅标记, 下半部处理
fn timer_handler() {
    TIMER_SOFTIRQ_pending.store(true, Ordering::Release);
}
fn timer_softirq() {
    let data = process_data();
    update_statistics(data);
}
```

---

## 6. 并发与原子操作纪律

多核环境下, 数据竞争和内存序错误极难复现 (可能运行 1000 次才触发一次).

### 6.1 原子操作 Ordering 选择

| 场景 | 正确 Ordering | 说明 |
|------|--------------|------|
| 简单标志位 (只关心最终值) | `Relaxed` | 无同步需求时最低开销 |
| 发布数据 + 保证可见性 | `Release` (写) + `Acquire` (读) | 生产者-消费者模式 |
| 需要全序 (如多变量一致性) | `SeqCst` | 最严格, 仅在必要时使用 |
| 自旋锁获取/释放 | `Acquire` / `Release` | 保护临界区 |

```rust
// ✗ 禁止 — 滥用 SeqCst
flag.store(true, Ordering::SeqCst);  // 只是标志位, 不需要全序

// ✓ 正确
flag.store(true, Ordering::Release);  // 发布侧用 Release
// ...
if flag.load(Ordering::Acquire) { ... }  // 消费侧用 Acquire
```

### 6.2 禁止的数据竞争模式

```rust
// ✗ 禁止 — 非原子读写共享变量
static mut COUNTER: u64 = 0;
fn increment() { unsafe { COUNTER += 1; } }  // 数据竞争

// ✓ 正确
static COUNTER: AtomicU64 = AtomicU64::new(0);
fn increment() { COUNTER.fetch_add(1, Ordering::Relaxed); }
```

### 6.3 自旋锁与中断的交互

```rust
// ✗ 禁止 — 持有自旋锁时不 disable IRQ, 可能被中断打断后死锁
let lock = MY_SPINLOCK.lock();
// ... 中断发生, ISR 尝试获取同一锁 → 死锁

// ✓ 正确
let _guard = MY_IRQ_LOCK.lock();  // IrqSpinLock 自动 disable IRQ
// ... 安全操作
```

---

## 7. 资源生命周期纪律

内核资源泄漏比用户态严重得多 — 操作系统无法回收泄漏的内核资源.

### 7.1 必须严格配对的操作

| 获取 | 释放 | 泄漏后果 |
|------|------|----------|
| `alloc_page()` | `free_page()` | 物理内存永久占用 |
| `lock()` / `lock_irq()` | drop guard | 死锁或中断永久禁用 |
| `register_irq()` | `unregister_irq()` | 中断处理函数悬挂 |
| `kmalloc()` | `kfree()` | 内核堆内存泄漏 |
| `VMA 插入` | `VMA 移除` | 虚拟地址空间泄漏 |

### 7.2 RAII 强制

所有内核资源获取必须通过 RAII guard, 不得手动调用释放函数:

```rust
// ✗ 禁止 — 手动释放, 异常路径可能遗漏
let ptr = kmalloc(size);
if ptr.is_null() { return Err(...); }
// ... 可能提前 return
kfree(ptr);  // 可能被跳过

// ✓ 正确 — RAII guard 自动释放
let guard = KmallocGuard::new(size)?;
// ... 任何退出路径都会 drop guard, 自动释放
```

### 7.3 失败路径回滚

资源分配链中某步失败时, 必须按 **LIFO 反序** 释放已分配的资源:

```rust
// 分配顺序: A → B → C
// 失败时释放: C → B → A (LIFO 反序)
let a = alloc_a()?;
let b = alloc_b()?;
let c = alloc_c()?;  // 失败
free_b(b);           // 反序释放
free_a(a);
```

---

## 8. 多架构兼容纪律

项目要求 x86_64 + aarch64 双架构通过. 新代码必须同时兼容.

### 8.1 cfg 使用规范

```rust
// ✗ 禁止 — 硬编码架构
fn read_cr3() -> u64 {
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3) }  // aarch64 上编译失败
}

// ✓ 正确 — cfg 分流
#[cfg(target_arch = "x86_64")]
fn read_cr3() -> u64 {
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3) }
}
#[cfg(target_arch = "aarch64")]
fn read_cr3() -> u64 {
    unsafe { core::arch::asm!("mrs {}, ttbr1_el1", out(reg) cr3) }
}
```

### 8.2 架构无关代码优先

能写架构无关代码的, 不写架构特定代码:

```rust
// ✗ 禁止 — 可以用原子操作解决的问题, 不需要 cfg
#[cfg(target_arch = "x86_64")]
static mut FLAG: bool = false;

// ✓ 正确 — 架构无关
static FLAG: AtomicBool = AtomicBool::new(false);
```

### 8.3 Arch trait 优先

新增架构特定功能时, 优先扩展 `Arch` trait, 而非在具体架构模块中添加独立函数.

---

## 9. 启动顺序依赖纪律

内核子系统有严格的初始化顺序. 新增子系统必须声明其依赖.

### 9.1 初始化顺序规则

```
arch → cpu → sync → mm (PMM→VMM→Slab) → irq → idt → timer → proc → fs → net → driver → syscall
```

后续子系统可依赖先前子系统的已初始化状态, 反之不可.

### 9.2 新增子系统初始化

新增子系统必须:
1. 在 `mod.rs` 头部注释声明依赖的子系统列表
2. 在 `init()` 函数中检查依赖子系统是否已初始化
3. 在合适的初始化链路中注册调用

```rust
// ✗ 禁止 — 假设 mm 已初始化
pub fn init() {
    let page = mm::alloc_page().unwrap();  // mm 可能还没初始化
}

// ✓ 正确 — 声明依赖 + 检查状态
/// 依赖: mm (PMM), sync
pub fn init() {
    assert!(mm::is_initialized(), "mm must init before my_subsystem");
    let page = mm::alloc_page().unwrap();
}
```

### 9.3 禁止全局静态初始化依赖

```rust
// ✗ 禁止 — 依赖其他子系统的全局初始化
static MY_TABLE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
// 在其他子系统的 init 中初始化 → 启动顺序不明确

// ✓ 正确 — 在自己的 init 中显式初始化
static MY_TABLE: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
pub fn init() {
    MY_TABLE.get_or_init(|| Mutex::new(Vec::new()));
}
```

---

## 10. 安全不变式自检

每次修改 framework 代码时, 必须逐项对照 6 条安全不变式 (I1-I6) 检查.

| # | 不变式 | 自检问题 |
|---|--------|----------|
| I1 | 内核态 CPU 状态不可被 services 篡改 | 本次是否暴露了新的 CPU 状态操作给 services? |
| I2 | 内核内存不可被 services 非法访问 | 本次是否让 services 能直接访问内核内存? |
| I3 | 用户态 CPU 状态只能通过 framework 安全入口修改 | 本次是否绕过了 usermode/userctx? |
| I4 | 用户内存只能通过 framework 安全代理访问 | 本次是否让 services 能直接引用用户内存? |
| I5 | 外设 MMIO/PIO 只能通过 framework 安全代理访问 | 本次是否让 services 能直接操作设备寄存器? |
| I6 | 外设 DMA 不可写入内核内存 | 本次是否让 DMA 能写入内核内存? |

**任何一项回答"是" = 本次修改违反安全不变式, 必须重新设计.**

---

## 11. 性能热路径纪律

调度器、中断处理、页错误处理等热路径有严格的性能约束.

### 11.1 热路径禁止操作

| 禁止操作 | 原因 |
|----------|------|
| 动态内存分配 (`kmalloc`/`alloc_page`) | 分配器可能触发回收/压缩, 不确定性延迟 |
| 持有 `Mutex` (可能 sleep) | 调度器热路径不可阻塞 |
| 不必要的函数调用层级 | 增加指令缓存压力 |
| 大数组/结构体栈上分配 | 栈空间有限 (通常 8-16KB) |
| 字符串格式化 (`format!`/`write!`) | 涉及分配和复杂逻辑 |

### 11.2 热路径允许操作

- 原子操作 (无锁)
- Per-CPU 变量访问 (无竞争)
- 小型固定大小数组 (栈上, ≤ 256 字节)
- 内联函数 (`#[inline(always)]`)
- 位运算 / 简单算术

### 11.3 调度器热路径约束

调度器的 `pick_next` / `tick_accounting` 路径:
- 执行时间目标: < 1μs
- 禁止任何可能阻塞的操作
- 禁止遍历链表超过 O(1) 深度 (除非是 per-CPU 队列)
- 锁持有时间目标: < 100ns

---

## 12. 提交检查清单

新代码提交前逐项确认:

**基础**:
- [ ] 双架构编译 0w0e
- [ ] 三审计通过
- [ ] host-tests 通过
- [ ] 文档同步更新 (CHANGELOG.md)

**耦合与编码**:
- [ ] 无新增 services 层 unsafe
- [ ] 无新增跨子系统内部访问
- [ ] 无新增循环依赖
- [ ] 无硬编码跨子系统常量
- [ ] 无顺手修改无关代码

**内核安全**:
- [ ] 中断上下文无 sleep 操作
- [ ] 原子操作 Ordering 正确
- [ ] 资源获取/释放严格配对
- [ ] 失败路径 LIFO 反序回滚
- [ ] framework 修改通过 I1-I6 自检

**多架构**:
- [ ] 架构特定代码有 `cfg` 门控
- [ ] 优先使用架构无关代码

---

## 13. 存量问题处理

已存在的不符合规范的代码, 按以下策略渐进修复:

1. **触及时修复**: 修改该模块时顺带修复
2. **标记待修**: 用 `// TODO(TCB): 策略可提取到 services` 标注
3. **禁止忽视**: 不允许以"历史遗留"为由永久搁置
4. **新代码零容忍**: 新代码必须 100% 符合本规范, 不允许"先写再改"

---

## 交叉引用

- [framekernel-dev-guide.md](./framekernel-dev-guide.md) — 架构开发场景指导
- [framekernel-nature.md](./framekernel-nature.md) — 安全不变式 I1-I6 定义
- [engineering-discipline.md](../plan/engineering-discipline.md) — 已有代码解耦进度
- [AGENTS.md](../../AGENTS.md) — 项目硬约束

---

- 创建: 2026-06-18
- 状态: 已落地
