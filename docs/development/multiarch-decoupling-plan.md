# AntX 多架构解耦工程规划书

> 版本: 1.0 | 日期: 2026-05-21 | 状态: Phase 4 完成

---

## 一、工程背景

### 1.1 现状

AntX 当前为 **x86_64 单架构内核**，架构耦合度极高：

| 耦合维度 | 数量 | 说明 |
|---------|------|------|
| `core::arch::asm!` 内联汇编 | **126 处** | 分散在 25 个 Rust 文件中，全为 x86 指令 |
| 纯汇编文件 | **5 个** | stage1/boot/entry/isr/switch，全部 `BITS 16/32/64` |
| 链接脚本 | **2 个** | `OUTPUT_FORMAT("elf64-x86-64")` 硬编码 |
| `arch::x86_64` 硬编码引用 | **10 处** | lib.rs、scheduler_ex.rs、smp/mod.rs 等直接导入 |
| `#[cfg(target_arch)]` 守卫 | **1 处** | 仅在 `arch/mod.rs`，无抽象层 |
| 构建目标 | **3 处配置** | `.cargo/config.toml`、Makefile、rust-toolchain 均固定 x86_64 |
| 用户态 syscall | **1 个文件** | `int 0x80` + x86_64 ABI 寄存器 |

### 1.2 目标

建立健壮的架构抽象层（Architecture Abstraction Layer, AAL），使内核**一套代码**可编译为 x86_64 和 aarch64 两个目标，切换仅需改动构建目标三元组。

### 1.3 原则

1. **主线不可破** — `main` 分支任何时刻必须编译通过、测试全绿
2. **渐进式、可回滚** — 每个阶段独立可验证，失败不回滚超过一个阶段
3. **x86_64 零退化** — 每阶段合并前，x86_64 行为与合并前完全一致
4. **抽象不泄漏** — 架构无关代码零 `asm!`、零 `#[cfg(target_arch)]`
5. **不盲从 Linux** — 命名和接口自洽，不照搬 `asm-generic` 或 Linux `arch/` 模式

---

## 二、分支策略

### 2.1 分支拓扑

```
origin/main (gitee)             ● 远程稳定基准
  │
main (本地)                     ● 始终跟踪 origin/main，可推送
  │
  ├──── feat/arch-abstraction   ● 解耦开发分支（长期存在，7 个 Phase 均在此进行）
  │       │
  │       ├── Phase 1 ──────── ● commit → MR → merge 回 main → 推送
  │       ├── Phase 2 ──────── ● (从 main 拉取最新) → commit → MR → merge
  │       ├── Phase 3 ──────── ● (同上)
  │       ├── Phase 4 ──────── ● ...
  │       ├── Phase 5 ──────── ● ...
  │       ├── Phase 6 ──────── ● ...
  │       └── Phase 7 ──────── ● ... → 最终合并，分支可归档
```

每个 Phase 遵循相同的 Git 工作流（见下文各 Phase 的"分支操作"小节）。

### 2.2 分支规范

| 规则 | 说明 |
|------|------|
| 所有开发在 `feat/arch-abstraction` | main 仅接收 Merge Commit，不直接 commit |
| 每个 Phase 结束时合并回 main | 确保 main 始终可构建、可测试 |
| Phase 开始前先 `git merge main` | 拉取上一 Phase 已合并的最新代码，避免冲突堆积 |
| main 推送前强制验证 | `cargo build` 0 error + `host-tests` 全通过 |
| 里程碑打 annotated tag | `git tag -a arch-p1 -m "Phase 1: Arch trait 定义完成"` |

### 2.3 合并闸门检查清单

在将 Phase 代码从 `feat/arch-abstraction` 合并到 `main` 之前，必须逐项确认：

```
[ ] cargo build --target x86_64-unknown-none       → 0 errors
[ ] cargo build --release --target ...               → 0 errors
[ ] host-tests                                       → 69/69 passed
[ ] git diff main -- src/kernel/arch/x86_64/         → 无功能退化
[ ] grep -rn 'unsafe' 新增量 ≤ 0 (或全部文档化)
[ ] 代码评审通过（至少 1 人）
```

### 2.4 分支操作速查

```bash
# ---- 首次：从 main 创建开发分支 ----
git checkout main
git pull origin main
git checkout -b feat/arch-abstraction
git push -u origin feat/arch-abstraction

# ---- 每个 Phase 开始时：同步 main 最新代码 ----
git checkout main
git pull origin main
git checkout feat/arch-abstraction
git merge main                    # 拉取上一 Phase 合并到 main 的更新

# ---- Phase 开发完成后：合并回 main ----
git checkout main
git pull origin main
git merge --no-ff feat/arch-abstraction   # --no-ff 保留分支历史
# 运行闸门检查...
git push origin main
git tag -a arch-p{N} -m "Phase {N} 完成"
git push origin arch-p{N}

# ---- 回退到 feat/arch-abstraction 继续下个 Phase ----
git checkout feat/arch-abstraction
```

---

## 三、目录结构规划

### 3.1 目标结构

```
src/kernel/
├── arch/
│   ├── mod.rs              ← Arch trait + 架构无关类型定义
│   ├── x86_64/
│   │   ├── mod.rs          ← impl Arch for X8664
│   │   ├── gdt.rs          ← x86 专有（不变）
│   │   ├── tss.rs          ← x86 专有（不变）
│   │   ├── apic.rs         ← x86 专有（不变）
│   │   └── ioapic.rs       ← x86 专有（不变）
│   │
│   └── aarch64/            ← 新增
│       ├── mod.rs           ← impl Arch for Aarch64
│       ├── gic.rs           ← ARM GICv3 中断控制器
│       ├── mmu.rs           ← ARM MMU 页表管理
│       ├── exception.rs     ← ARM 异常向量表
│       └── psci.rs          ← ARM PSCI 电源管理
│
├── proc/
│   ├── switch.rs            ← 架构相关：调用 arch::context_switch()
│   ├── switch.asm           ← 移至 arch/x86_64/context.asm
│   └── ...
│
├── boot/
│   ├── {arch}/
│   │   ├── x86_64/
│   │   │   ├── stage1.asm
│   │   │   ├── boot.asm
│   │   │   ├── entry.asm
│   │   │   └── multiboot.rs
│   │   └── aarch64/
│   │       ├── start.S
│   │       └── entry.rs
│   └── mod.rs               ← 引导层抽象 + reset 入口
│
├── idt/
│   └── ...                  ← x86 特有，改名或下沉至 arch/x86_64/interrupt/
│
├── mm/
│   ├── arch.rs              ← 架构无关页表操作接口
│   ├── vmm.rs               ← 移除所有 asm!，调用 arch::mmu_*
│   └── ...
│
├── sync/
│   ├── arch.rs              ← arch::spin_hint(), arch::interrupt_save()
│   └── ...
│
├── cpu/
│   ├── arch.rs              ← arch::cpu_id(), arch::timestamp()
│   └── ...
│
├── link/
│   ├── x86_64.ld            ← 从 src/link.ld 移入
│   └── aarch64.ld           ← 新增
│
└── lib/                     ← C 运行时库（不动）
    ├── types.h
    ├── string.h
    ├── string.c
    ├── string.rs
    └── mod.rs
```

### 3.2 汇编文件归属原则

| 原则 | 说明 |
|------|------|
| 架构专有汇编 | 放 `arch/{arch}/` 下，如 `context.asm`、`exception.S` |
| 引导阶段汇编 | 放 `boot/{arch}/` 下，如 `stage1.asm`、`start.S` |
| 不得跨架构 import | `arch/x86_64/` 和 `arch/aarch64/` 永不互相引用 |

### 3.3 链接脚本管理

```
src/link.ld              ← 删除（硬编码 x86_64）
src/kernel/link/
├── x86_64.ld            ← 从原 link.ld 迁移，稍作调整
└── aarch64.ld           ← 新增
```

Makefile 中根据 `ARCH ?= x86_64` 选择对应链接脚本。

---

## 四、代码架构设计

### 4.1 Arch trait 定义

```rust
// src/kernel/arch/mod.rs

/// 架构抽象层入口 trait
/// 每个架构提供各自的零大小类型实现此 trait
pub trait Arch: Send + Sync + 'static {
    // ========== 中断控制 ==========

    /// 禁用中断，返回保存的 flags
    fn interrupt_disable() -> usize;
    /// 恢复中断状态
    fn interrupt_restore(flags: usize);
    /// 启用中断
    fn interrupt_enable();
    /// 判断当前是否处于中断上下文
    fn is_interrupt_enabled() -> bool;
    /// CPU 暂停等待中断
    fn halt();

    // ========== 页表 / MMU ==========

    /// 刷新 TLB 中指定虚拟地址
    fn tlb_flush_page(vaddr: usize);
    /// 刷新全部 TLB
    fn tlb_flush_all();
    /// 读取当前页表物理基址
    fn read_page_table_base() -> u64;
    /// 切换页表
    fn write_page_table_base(paddr: u64);
    /// 读取页故障地址
    fn read_fault_address() -> usize;

    // ========== 上下文切换 ==========

    /// 切换到目标进程上下文
    /// 调用方确保 from 和 to 指向有效的 Context
    unsafe fn context_switch(from: *mut Context, to: *const Context);
    /// 首次进入用户态（fork 出的新进程）
    unsafe fn enter_user(entry: usize, stack: usize, arg: usize) -> !;
    /// 从内核态返回到用户态
    unsafe fn return_to_user();

    // ========== CPU / 时间 ==========

    /// 获取当前 CPU 标识
    fn cpu_id() -> u32;
    /// 获取高精度时间戳（单调递增）
    fn timestamp() -> u64;
    /// 内存屏障：全屏障
    fn fence();
    /// 内存屏障：写屏障
    fn fence_w();

    // ========== SMP / IPI ==========

    /// 发送核间中断
    fn send_ipi(target_cpu: u32, vector: u8);
    /// 广播核间中断到所有 CPU
    fn broadcast_ipi(vector: u8);

    // ========== I/O ==========

    /// 端口输出 8-bit
    unsafe fn outb(port: u16, value: u8);
    /// 端口输入 8-bit
    unsafe fn inb(port: u16) -> u8;
    /// 端口输出 32-bit
    unsafe fn outl(port: u16, value: u32);
    /// 端口输入 32-bit
    unsafe fn inl(port: u16) -> u32;

    // ========== 系统重置 ==========

    /// 关机
    fn shutdown() -> !;
    /// 重启
    fn reboot() -> !;
}

/// 编译时选择的架构类型
#[cfg(target_arch = "x86_64")]
pub type CurrentArch = x86_64::X8664;

#[cfg(target_arch = "aarch64")]
pub type CurrentArch = aarch64::Aarch64;

/// 便捷调用宏：等价于 <CurrentArch as Arch>::method()
macro_rules! arch {
    ($method:ident $(, $arg:expr)*) => {
        <$crate::kernel::arch::CurrentArch as $crate::kernel::arch::Arch>::$method($($arg),*)
    };
}
```

### 4.2 上下文结构体

```rust
// src/kernel/proc/types.rs（架构无关部分）

#[repr(C)]
pub struct Context {
    /// 通用寄存器组（架构定义）
    pub regs: ArchRegs,
    /// 页表物理基址
    pub page_table: u64,
    /// 指令指针
    pub instruction_pointer: usize,
    /// 栈指针
    pub stack_pointer: usize,
    /// 状态标志
    pub flags: usize,
}

// 在 arch/mod.rs 中由各架构定义
#[cfg(target_arch = "x86_64")]
pub type ArchRegs = [u64; 15];  // r15..rbx

#[cfg(target_arch = "aarch64")]
pub type ArchRegs = [u64; 31];  // x0..x30
```

### 4.3 模块迁移映射表

以下列出每个需要改造的模块及改造方式：

| 原模块 | 当前 asm! 数 | 改造方式 |
|--------|-------------|---------|
| `sync/spinlock.rs` | 3 | `arch!(interrupt_disable)` + `arch!(fence)` 替代 `pushfq/cli` |
| `sync/mutex.rs` | 1 | `arch!(timestamp)` 替代 `rdtsc` |
| `sync/types.rs` | 1 | `arch!()` 或移入 `arch/` |
| `proc/scheduler.rs` | 5 | `arch!(interrupt_disable/enable/halt)` 替代 `cli/sti/hlt` |
| `proc/scheduler_ex.rs` | 1 | `arch::x86_64::tss::*` → Arch trait 方法 |
| `proc/switch.asm` | 全文件 | 迁移至 `arch/x86_64/context.asm` |
| `proc/user_proc.rs` | 2 | `enter_user` 走 Arch trait |
| `mm/vmm.rs` | 6 | `arch!(tlb_flush_page)` + `arch!(write_page_table_base)` |
| `mm/mod.rs` | — | KERNEL_BASE 等常量根据 `target_arch` 取值 |
| `idt/` 全部 | 8 | 下沉至 `arch/x86_64/interrupt.rs` |
| `timer/pit.rs` | 2 | x86 专有，不移除但标记 deprecated |
| `timer/irq.rs` | — | PIC 重映射下沉至 arch |
| `klog/mod.rs` | 5 | 串口抽象为 `arch::SerialPort` trait |
| `cpu/cpuid.rs` | 1 | x86 专有，`#[cfg(target_arch)]` 守卫 |
| `cpu/mod.rs` | 2 | FPU 初始化下沉至 arch |
| `driver/framework.rs` | 若干 | I/O 端口走 `arch!(inb/outb)` |
| `driver/char/vga.rs` | 2 | x86 专有 |
| `barrier/reset/bhr.rs` | 5 | 重置走 Arch trait |
| `barrier/reset/bsr.rs` | 1 | 同上 |
| `ipc/async_ipc.rs` | 2 | 上下文切换走 Arch trait |
| `pwid/*.rs` | 3 | `_rdtsc()` → `arch!(timestamp)` |
| `lib/string.rs` | 1 | 架构无关的 Rust 实现，移除 asm! |
| `net/types.rs` | 2 | lwIP 临界区适配，走 arch! |
| `user/lib/src/sys.rs` | 3 | `int 0x80` → arch 定义的系统调用指令 |

### 4.4 禁止模式清单

以下模式在迁移完成后应零存在：

| 禁止模式 | grep 检测命令 |
|---------|-------------|
| 硬编码 `arch::x86_64::` | `grep -rn 'arch::x86_64' --include='*.rs' src/kernel/` (排除 arch/ 自身) |
| 裸 `core::arch::asm!` | `grep -rn 'asm!\b' --include='*.rs' src/kernel/` (排除 arch/ 自身) |
| 裸 `core::arch::x86_64` | `grep -rn 'x86_64::' --include='*.rs' src/kernel/` |
| 硬编码 `0xFFFF800000000000` | `grep -rn '0xFFFF8' --include='*.rs' src/kernel/` |
| 硬编码 `int 0x80` | `grep -rn '0x80' --include='*.rs' src/user/` |

---

## 五、分阶段实施计划

> 每个 Phase 均包含 "分支操作" 小节，明确 Git 命令步骤。

---

### Phase 1：Arch trait 骨架 + 类型系统（预计 1-2 天）

#### 分支操作

```bash
# Step 1: 从 main 创建 feat/arch-abstraction 分支
git checkout main
git pull origin main           # 确保 main 是最新的
git checkout -b feat/arch-abstraction
git push -u origin feat/arch-abstraction

# Step 2: 在 feat/arch-abstraction 上开发...
# (编码阶段)

# Step 3: Phase 1 完成后，合并回 main
git checkout main
git pull origin main           # 防止远程有新提交
git merge --no-ff feat/arch-abstraction

# Step 4: 闸门验证
cargo build --target x86_64-unknown-none
cargo build --target aarch64-unknown-none
cargo test --manifest-path host-tests/Cargo.toml

# Step 5: 推送 + 打标签
git push origin main
git tag -a arch-p1 -m "Phase 1: Arch trait 定义完成"
git push origin arch-p1

# Step 6: 回到开发分支，继续 Phase 2
git checkout feat/arch-abstraction
git merge main                 # 同步刚刚合并的内容
```

#### 目标

定义完整 Arch trait，不动现有代码。仅新增文件，不修改任何现有模块。

#### 产出

| 文件 | 内容 |
|------|------|
| `src/kernel/arch/mod.rs` | Arch trait 定义 + `CurrentArch` 类型别名 + `arch!` 宏 |
| `src/kernel/arch/x86_64/mod.rs` | `pub struct X8664;` (空壳，Phase 2 填肉) |
| `src/kernel/arch/aarch64/mod.rs` | `pub struct Aarch64;` (stub，所有方法 `unimplemented!()`) |
| `src/kernel/cpu/arch.rs` | `arch::timestamp()`、`arch::cpu_id()` 的 CPU 层面封装 |
| `src/kernel/sync/arch.rs` | `arch::spin_hint()`、`arch::interrupt_save()` 封装 |
| `src/kernel/mm/arch.rs` | `arch::tlb_flush_page()` 等封装 |

#### 验证

```
[ ] cargo build --target x86_64-unknown-none     → 0 errors
[ ] cargo build --target aarch64-unknown-none     → 编译通过（不运行）
[ ] host-tests                                     → 69/69
```

> **关键约束**：Phase 1 零修改现有代码。所有新增文件即使未被引用，也应能独立编译。

---

### Phase 2：x86_64 实现 Arch trait（预计 2-3 天）

#### 分支操作

```bash
# Phase 2 开始前：从 main 同步最新代码
git checkout main
git pull origin main
git checkout feat/arch-abstraction
git merge main

# 开发完成后：合并 + 验证 + 推送
git checkout main
git pull origin main
git merge --no-ff feat/arch-abstraction
# 运行闸门检查...
git push origin main
git tag -a arch-p2 -m "Phase 2: x86_64 实现 Arch trait"
git push origin arch-p2

# 回到开发分支
git checkout feat/arch-abstraction
git merge main
```

#### 目标

给 `impl Arch for X8664` 填肉，将现有 x86_64 代码中的 `asm!` 和硬件操作封装为 trait 方法。**此时不迁移调用方**，只提供封装。

#### 操作要点

| 方法组 | 封装内容 | 原实现位置 |
|--------|---------|-----------|
| 中断控制 | `pushfq; pop; cli` → `interrupt_disable() -> Rflags` | `spinlock.rs` `scheduler.rs` 等 |
| 页表 / MMU | `mov cr3`、`invlpg`、CR2 读取 | `vmm.rs` |
| 上下文切换 | `switch.asm` → `arch/x86_64/context.asm` | `switch.asm` (全文件) |
| CPU / 时间 | `rdtsc`、`cpuid` | `cpuid.rs`、`pwid/` |
| 内存屏障 | `mfence`、`sfence`、`lfence` | 各处 |
| SMP / IPI | `apic::send_ipi()` 等 | `smp/mod.rs` |
| I/O 端口 | `inb`/`outb` 等 | `driver/framework.rs` |
| 系统重置 | 键盘控制器、triple fault | `barrier/reset/bhr.rs` |

每个方法用 `#[inline(always)]` 标记，确保零开销。`gdt_init()` 保持为 x86_64 内部函数，不加到 trait（GDT 是 x86 特有）。

#### 验证

同 Phase 1。额外验证：

```
[x] cargo build --target x86_64-unknown-none     → 0 errors (40 existing warnings, 0 new)
[x] cargo build --target aarch64-unknown-none     → 编译通过（不运行）
[x] host-tests                                     → 69/69
[x] x86_64 binary 与 Phase 1 一致 (未迁移调用方)
```

---

### Phase 3：内核模块逐步迁移（预计 5-7 天）

#### 分支操作

```bash
# Phase 3 开始前
git checkout main && git pull origin main
git checkout feat/arch-abstraction && git merge main

# 由于 Phase 3 周期较长(5-7天)，建议内部再细分 3.1 ~ 3.4 子阶段
# 每个子阶段完成后建议 commit 并 push 到远程 feat/arch-abstraction
# (但不合并到 main，待整个 Phase 3 完成后再合并)

# Phase 3 整体完成后合并
git checkout main && git pull origin main
git merge --no-ff feat/arch-abstraction
# 闸门检查...
git push origin main
git tag -a arch-p3 -m "Phase 3: 内核模块迁移完成"
git push origin arch-p3
git checkout feat/arch-abstraction && git merge main
```

#### 目标

逐一将各模块的硬编码 x86_64 调用改为 Arch trait 调用。

#### 迁移顺序（按依赖关系从底层到高层，严格串行）

| 步骤 | 模块 | 改造内容 | 预计改动文件数 |
|------|------|---------|-------------|
| 3.1 | `sync/` | 自旋锁、互斥锁中 `cli/pause` → `arch!()` | 3 |
| 3.2 | `cpu/` | `cpuid`、`rdtsc`、FPU 初始化 → `arch!()` | 3 |
| 3.3 | `mm/` | `invlpg`、`mov cr3`、KERNEL_BASE 条件编译 | 3 |
| 3.4 | `idt/` | 下沉至 `arch/x86_64/interrupt.rs` | 4 |
| 3.5 | `timer/` | PIT 保持，抽象 timer 接口 | 2 |
| 3.6 | `proc/` | 上下文切换、用户态进入、TSS 操作 | 4 |
| 3.7 | `smp/` | IPI 调用 `arch!()` | 1 |
| 3.8 | `boot/` | Multiboot 保留，抽象引导参数传递 | 2 |
| 3.9 | `klog/` | 串口驱动抽象 | 1 |
| 3.10 | `driver/` | I/O 端口 `arch!(inb/outb)` | 3 |
| 3.11 | `pwid/` | `_rdtsc()` → `arch!(timestamp)` | 2 |
| 3.12 | `barrier/` | 系统重置 `arch!(shutdown/reboot)` | 2 |
| 3.13 | `user/` | 系统调用指令抽象 | 1 |

#### 验证清单

```
[x] 3.1  sync/       — spinlock/mutex/mutex_types: cli/pause/rdtsc → arch!()
[x] 3.2  cpu/        — tsc(cpuid+rdtsc)→arch!(timestamp()), cpuid/msr/fpu 保留(x86特有)
[x] 3.3  mm/         — read_cr3/write_cr3/invlpg → arch!()
[x] 3.4  idt/        — fault_address/timestamp/fence/inb/outb/halt → arch!()
[x] 3.5  timer/      — PIT outb/inb → arch!()
[x] 3.6  proc/       — context_switch/enter_user/halt/cli/sti → arch!()
[x] 3.7  smp/        — IPI cli/sti → arch!() (in scheduler.rs)
[x] 3.8  boot/       — Multiboot 保留, 无需架构抽象
[x] 3.9  klog/       — outb/inb/rdtsc/interrupt_disable/interrupt_restore → arch!()
[x] 3.10 driver/     — outb/inb/outl/inl/sfence → arch!() (outw/inw/VGA 保留,x86特有)
[x] 3.11 pwid/       — _rdtsc() → arch!(timestamp())
[x] 3.12 barrier/    — cli/outb/halt → arch!() (triple_fault 保留,x86特有)
[x] 3.13 user/       — iretq → arch!(enter_user())
[x] cargo build --target x86_64-unknown-none     → 0 errors
[x] 额外覆盖: net/types(SysProt cli/sti)、dma/ffi(sfence/rdtsc)、dma/engine(sfence)、
             ahci(sfence)、ipc(async_ipc/scheduler_integration rdtsc)、fs/ramfs(rdtsc)、
             pci(outb/inb/outl/inl)、e1000(outb/inb)
[x] 不可迁移保留: lfence(无等价Arch方法)、rep stosb(字符串指令)、lidt/lgdt(GDT特有)、
                 rdmsr/wrmsr(MSR)、cpuid、cr0/cr4/fninit(FPU)、VGA光标、triple_fault
```

#### 禁用模式检查（每个子步骤后执行）

```bash
# 除 arch/ 自身外，架构无关代码中不应再出现
grep -rn 'arch::x86_64' --include='*.rs' src/kernel/ | grep -v 'src/kernel/arch/'
grep -rn 'asm!\b' --include='*.rs' src/kernel/ | grep -v 'src/kernel/arch/'
```

> **铁律**：每迁移一个模块立即 `cargo build` 验证。严禁批量迁移后面对百级编译错误。

---

### Phase 4：构建系统多目标化（预计 1-2 天）

#### 分支操作

```bash
git checkout main && git pull origin main
git checkout feat/arch-abstraction && git merge main

# 开发...

git checkout main && git pull origin main
git merge --no-ff feat/arch-abstraction
# 闸门检查...
git push origin main
git tag -a arch-p4 -m "Phase 4: 构建系统多目标化"
git push origin arch-p4
git checkout feat/arch-abstraction && git merge main
```

#### 目标

一键切换架构。`make ARCH=x86_64` 和 `make ARCH=aarch64` 均能编译。

#### 改动

**Makefile**：新增 `ARCH ?= x86_64` 变量块
```makefile
ARCH ?= x86_64

ifeq ($(ARCH),aarch64)
    CC = aarch64-linux-gnu-gcc
    LD = aarch64-linux-gnu-ld
    AS = aarch64-linux-gnu-as
    RUST_TARGET = aarch64-unknown-none
    QEMU = qemu-system-aarch64
    QEMU_MACHINE = virt
    QEMU_CPU = cortex-a72
    LDSCRIPT = src/kernel/link/aarch64.ld
else
    # 保持现有 x86_64 全部配置不变
    CC = x86_64-linux-gnu-gcc
    LD = x86_64-linux-gnu-ld
    AS = nasm
    RUST_TARGET = x86_64-unknown-none
    QEMU = qemu-system-x86_64
    QEMU_CPU ?= qemu64
    LDSCRIPT = src/kernel/link/x86_64.ld
endif
```

**链接脚本迁移**：
- `src/link.ld` → 移动到 `src/kernel/link/x86_64.ld`
- 新增 `src/kernel/link/aarch64.ld`

**Cargo 配置**：
- `src/rust/.cargo/config.toml`：移除硬编码 `target = "x86_64-unknown-none"`
- `src/user/.cargo/config.toml`：同上

#### 验证

```
[ ] make ARCH=x86_64 all           → 成功，行为与 Phase 3 完全一致
[ ] make ARCH=aarch64 all          → 编译通过（不运行）
[ ] host-tests (x86_64)             → 69/69
```

---

### Phase 5：aarch64 stub 实现（预计 3-4 天）

#### 分支操作

```bash
# 这是 arm64 代码首次进入仓库的 Phase
git checkout main && git pull origin main
git checkout feat/arch-abstraction && git merge main

# 开发...
# 建议每完成一个组件 (mmu/exception/gic) 就 commit 一次
# commit message 示例: "feat(aarch64): MMU identity mapping 初始化"

git checkout main && git pull origin main
git merge --no-ff feat/arch-abstraction
git push origin main
git tag -a arch-p5 -m "Phase 5: aarch64 stub 实现"
git push origin arch-p5
git checkout feat/arch-abstraction && git merge main
```

#### 目标

arm64 基础设施到位，QEMU 中能进入内核入口并打印串口日志（即使随后 panic）。

#### 产出

| 文件 | 内容 |
|------|------|
| `src/kernel/boot/aarch64/start.S` | UEFI/DeviceTree 启动入口 (EL3→EL2→EL1) |
| `src/kernel/boot/aarch64/entry.rs` | Rust 入口，调用 kernel_init |
| `src/kernel/arch/aarch64/mmu.rs` | identity mapping 页表初始化 |
| `src/kernel/arch/aarch64/exception.rs` | 异常向量表 (EL1h/EL1t/EL0) + 默认 handler |
| `src/kernel/arch/aarch64/gic.rs` | GICv3 初始化（至少能响应 timer 中断） |
| `src/kernel/arch/aarch64/psci.rs` | PSCI 关机/重启 |

#### 验证

```
[ ] make ARCH=x86_64 all           → 成功，host-tests 69/69
[ ] make ARCH=aarch64 run          → QEMU 输出 "QueenX starting" 到串口
```

---

### Phase 6：aarch64 完整实现（预计 5-8 天）

#### 分支操作

```bash
# 这是最长的 Phase，建议细分 6.1~6.5 子阶段并经常 push
git checkout main && git pull origin main
git checkout feat/arch-abstraction && git merge main

# 开发... (5-8天)

git checkout main && git pull origin main
git merge --no-ff feat/arch-abstraction
git push origin main
git tag -a arch-p6 -m "Phase 6: aarch64 完整实现"
git push origin arch-p6
git checkout feat/arch-abstraction && git merge main
```

#### 目标

arm64 能 boot 到 Shell，功能与 x86_64 对齐。

#### 子阶段

| 子阶段 | 内容 | 可验证标准 |
|--------|------|-----------|
| 6.1 MMU | TTBR0_EL1/TTBR1_EL1 页表映射，4KB/2MB 页面 | `mmu_init()` 后读写物理地址正常 |
| 6.2 上下文 | 上下文切换 `cpu_context` (x0-x30 + sp + lr) | 两个线程 ping-pong 切换正常工作 |
| 6.3 用户态 | `eret` EL0 进入，`svc #0` 系统调用 | 用户程序触发 syscall 后正确返回 |
| 6.4 中断 | GICv3 中断分发 + Generic Timer | timer 中断定期触发 |
| 6.5 外设 | PL011 串口、virtio-blk 磁盘 | Shell 交互、文件读写正常 |

#### 验证

```
[ ] make ARCH=x86_64 all           → host-tests 69/69
[ ] make ARCH=aarch64 run          → QEMU 进入 Shell
[ ] osinfo / ps / ls / echo 命令正常
[ ] HvFS 持久化读写正常 (arm64 上 host-tests 等价验证)
```

---

### Phase 7：测试完善 + 文档（预计 2-3 天）

#### 分支操作

```bash
git checkout main && git pull origin main
git checkout feat/arch-abstraction && git merge main

# 开发...

git checkout main && git pull origin main
git merge --no-ff feat/arch-abstraction
git push origin main
git tag -a arch-p7 -m "Phase 7: 测试完善 + 文档"
git push origin arch-p7

# feat/arch-abstraction 分支可归档
git branch -d feat/arch-abstraction
git push origin --delete feat/arch-abstraction
```

#### 产出

| 产出 | 内容 |
|------|------|
| 架构无关测试 | host-tests 中新增 trait 方法单元测试 |
| 双架构 CI | CI 脚本同时跑 x86_64 + aarch64 构建 |
| 移植指南 | `docs/development/arch-porting-guide.md` — 给未来 riscv64 等架构 |
| 本规划书 | 更新为 "已完成" 状态，记录实际耗时与踩坑记录 |
| 双架构 README | 更新 `README.md`，标注支持 x86_64 + aarch64 |

#### 最终验证

```
[ ] make ARCH=x86_64 test-host    → 69/69
[ ] make ARCH=aarch64 test-host   → 架构无关测试通过
[ ] 全量 grep 检查禁止模式为零
```

---

## 六、编码规范

### 6.1 内联汇编处理

```rust
// ❌ 禁止
unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

// ✅ 正确
crate::kernel::arch::CurrentArch::interrupt_disable();

// 或通过宏
arch!(interrupt_disable);
```

### 6.2 条件编译

```rust
// ❌ 禁止：在架构无关代码中使用 cfg
#[cfg(target_arch = "x86_64")]
fn foo() { ... }
#[cfg(target_arch = "aarch64")]
fn foo() { ... }

// ✅ 正确：Arich trait 统一接口
fn foo() {
    let ts = arch!(timestamp);
    // 架构无关逻辑
}
```

### 6.3 常量定义

```rust
// ❌ 禁止：架构无关代码中硬编码
let kernel_base = 0xFFFF800000000000u64;

// ✅ 正确：从 arch 模块获取
use crate::kernel::arch::KERNEL_VIRT_BASE;
// 或 arch!() 运行时获取
```

### 6.4 测试规范

```rust
/// 每个新抽象接口必须包含单元测试
#[test]
fn test_interrupt_disable_restore() {
    let flags = arch!(interrupt_disable);
    assert!(!arch!(is_interrupt_enabled));
    arch!(interrupt_restore, flags);
    // 注意：QEMU 中 IF 状态可能不精确
}
```

---

## 七、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| Arch trait 设计不当需大改 | 中 | 高 | Phase 1-2 充分评审后再推进 Phase 3 |
| `#[inline(always)]` 未生效导致性能退化 | 低 | 中 | Phase 2 后跑 chaos 性能基准对比 |
| 上下文切换寄存器保存不完整 | 中 | 高 | 每个架构的 context.asm 独立单元测试 |
| arm64 页表映射错误 | 高 | 中 | 先用 identity mapping 验证，再逐步加 |
| 第三方 lwIP C 代码的 `inb/outb` 等 | 中 | 低 | 保持 C 层不变，仅 Rust 侧抽象 |
| 用户态 ABI 不兼容 | 中 | 中 | Phase 6 重新编译用户态程序 |

---

## 八、参考

- Rust Embedded Book: Portable Targets
- ARM Architecture Reference Manual (Armv8-A)
- Zephyr RTOS arch/ 层设计（但不是标准，AntX 自洽即可）
- 本项目的 `CONTRIBUTING` 与 `coding-style.md`

---

> 此文档存放于 `docs/development/multiarch-decoupling-plan.md`
> 执行过程中如有重大决策变更，请更新本文档并记录在 Phase 对应小节下。
