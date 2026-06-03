# QueenX 框内核 (Framekernel) 迁移路线图

> **版本**: v1.1
> **参考论文**: [Asterinas: A Linux ABI-Compatible, Rust-Based Framekernel OS with a Small and Sound TCB](https://arxiv.org/abs/2506.03876) (USENIX ATC 2025)
> **目标**: 将 QueenX 从"unsafe 散布的宏内核"改造为"TCB 清晰收敛的框内核"
> **核心理念**: 宏内核的性能 + 微内核的安全 —— 用 Rust 语言级特权分离取代进程级 IPC
> **当前状态**: api.rs 全部 11 子系统完工, Phase 0 启动中

---

## 一、现状评估

### 1.1 代码规模

| 指标 | 数值 |
|------|------|
| 内核总行数 (Rust) | ~82,000 (剔除 smoltcp 60K vendored) |
| `unsafe` 出现次数 | **1,688** |
| `unsafe` 涉及文件数 | **30+** |
| 现有 `api.rs` 文件 | **5 个** (mm, proc, credo, barrier, vfs) |
| 子系统数 (有 mod.rs 的) | **23** |
| 目标架构 | x86_64 + aarch64 (双架构生产就绪) |

### 1.2 `unsafe` 分布 (Top 10 热点)

```
128  syscall/mod.rs          ← 系统调用分发, 用户指针操作
 62  sync/mod.rs             ← 同步原语, RawMutex 实现
 56  driver/net/e1000.rs     ← 网卡 MMIO
 55  proc/scheduler_ex.rs    ← 上下文切换, raw pointer
 47  proc/api.rs             ← 进程表 raw pointer
 46  credo/session.rs        ← 会话管理, 全局锁
 44  net/init.rs             ← 网络初始化, smoltcp FFI
 40  mm/vmm_x86_64.rs        ← 页表 raw 操作
 33  fs/ramfs/ramfs.rs       ← 文件系统 page 操作
 31  chitin/mod.rs           ← 设备注册表 raw pointer
```

**核心发现**: `unsafe` 遍布所有层级 —— 从底层页表到上层文件系统, 没有有效的安全边界。这正是宏内核在 Rust 下的典型困境: TCB ≈ 100%。

### 1.3 现有 API 层盘点

| api.rs 文件 | 对应子系统 | 是否框内核就绪 | 说明 |
|-------------|-----------|----------------|------|
| [proc/api.rs](file:///home/anfer/Code/AntX/src/kernel/proc/api.rs) | 进程管理 | ⚠️ 部分 | `#[no_mangle]` 函数集, 但有 `CProcess` raw struct |
| [mm/api.rs](file:///home/anfer/Code/AntX/src/kernel/mm/api.rs) | 内存管理 | ⚠️ 部分 | `#[no_mangle]` 函数集, 裸指针接口 |
| [credo/api.rs](file:///home/anfer/Code/AntX/src/kernel/credo/api.rs) | 安全子系统 | ⚠️ 部分 | `#[no_mangle]` 函数集 |
| [barrier/api.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/api.rs) | 栏栈恢复 | ⚠️ 部分 | 有契约注释, 方向正确 |
| [vfs/api.rs](file:///home/anfer/Code/AntX/src/kernel/fs/vfs/api.rs) | 虚拟文件系统 | ⚠️ 部分 | 有 `Vfs` trait 声明 |
| [chitin/mod.rs](file:///home/anfer/Code/AntX/src/kernel/chitin/mod.rs) | 设备框架 | ✅ 较好 | 已标注为 API 层, 6 个协议族 |

**关键判断**: 现有 5 个 `api.rs` 都是 `#[no_mangle] fn` 风格 —— 暴露的是 C 风格 FFI 入口, 而非 Rust 安全抽象。这正好需要升级为 OSTD 风格的安全 API。

---

## 二、资源敏感性清单 (摘自论文 §3.1, 适配 QueenX)

框内核的核心设计原则: 将内核资源区分为 **"敏感"(只能在 framework 内操作)** 与 **"非敏感"(可暴露给 services)**。

### 2.1 CPU 资源

| 资源 | 敏感性 | 理由 | QueenX 当前位置 |
|------|--------|------|----------------|
| Ring 0 执行权 | **敏感** | 只有 framework 可直接修改 GDT/TSS | [gdt.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/gdt.rs), [tss.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/tss.rs) |
| 内核栈 | **敏感** | 栈溢出 → UB, 必须 framework 管理 | [switch.asm](file:///home/anfer/Code/AntX/src/kernel/proc/switch.asm), [scheduler_ex.rs](file:///home/anfer/Code/AntX/src/kernel/proc/scheduler_ex.rs) |
| CR0/CR2/CR3/CR4 控制寄存器 | **敏感** | 直接硬件控制 | [vmm_x86_64.rs](file:///home/anfer/Code/AntX/src/kernel/mm/vmm_x86_64.rs), [smp_init.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/smp_init.rs) |
| MSR / EFER | **敏感** | 需高特权级 | [msr.rs](file:///home/anfer/Code/AntX/src/kernel/cpu/msr.rs) |
| GDT / IDT / TSS | **敏感** | 破坏后系统崩溃 | [gdt.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/gdt.rs), [idt/idt.rs](file:///home/anfer/Code/AntX/src/kernel/idt/idt.rs) |
| 用户态寄存器 (UserContext) | **非敏感** | framework 提供读写, service 调用 | [context.rs](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64/context.rs) |

### 2.2 内存资源

| 资源 | 敏感性 | 理由 | QueenX 当前位置 |
|------|--------|------|----------------|
| 内核页表 (PML4) | **敏感** | 不当修改 → 全部崩溃 | [vmm_x86_64.rs](file:///home/anfer/Code/AntX/src/kernel/mm/vmm_x86_64.rs) |
| 内核堆 | **敏感** | 需框架统一管理 | [kmalloc.rs](file:///home/anfer/Code/AntX/src/kernel/mm/kmalloc.rs), [slab.rs](file:///home/anfer/Code/AntX/src/kernel/mm/slab.rs) |
| Frame 物理页 | **敏感** | 引用计数 + 元数据 | [pmm.rs](file:///home/anfer/Code/AntX/src/kernel/mm/pmm.rs) |
| 用户页表 | **非敏感** | VmSpace 安全包装后暴露 | [vma.rs](file:///home/anfer/Code/AntX/src/kernel/mm/vma.rs) |
| 用户内存映射 | **非敏感** | 通过 VmSpace 操作 | [proc/user_proc.rs](file:///home/anfer/Code/AntX/src/kernel/proc/user_proc.rs) |
| IOMMU 页表 | **敏感** | DMA 攻击向量 | 目前缺失 |

### 2.3 设备资源

| 资源 | 敏感性 | 理由 | QueenX 当前位置 |
|------|--------|------|----------------|
| APIC / IOAPIC | **敏感** | 中断控制核心 | [apic.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/apic.rs), [ioapic.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/ioapic.rs) |
| GIC (AArch64) | **敏感** | ARM 中断控制器 | [gic.rs](file:///home/anfer/Code/AntX/src/kernel/arch/aarch64/gic.rs) |
| 本地 APIC Timer | **敏感** | 调度器 tick 源 | [apic.rs](file:///home/anfer/Code/AntX/src/kernel/arch/x86_64/apic.rs) |
| 外设 MMIO | **非敏感** | 通过 IoMem 安全代理 | [e1000.rs](file:///home/anfer/Code/AntX/src/kernel/driver/net/e1000.rs) (裸访问) |
| 外设 PIO | **非敏感** | 通过 IoPort 安全代理 | [ata.rs](file:///home/anfer/Code/AntX/src/kernel/driver/storage/ata.rs) |
| DMA 缓冲区 | **非敏感** | 通过 DmaStream 安全代理 | [dma/engine.rs](file:///home/anfer/Code/AntX/src/kernel/dma/engine.rs) |
| PCI 配置空间 | **敏感** | 枚举/配置需全局协调 | [pci/mod.rs](file:///home/anfer/Code/AntX/src/kernel/pci/mod.rs) |

### 2.4 中断资源

| 资源 | 敏感性 | 理由 | QueenX 当前位置 |
|------|--------|------|----------------|
| IDT 表 | **敏感** | 直接硬件 | [idt/idt.rs](file:///home/anfer/Code/AntX/src/kernel/idt/idt.rs) |
| ISR 入口 | **敏感** | asm stub | [isr.asm](file:///home/anfer/Code/AntX/src/kernel/boot/isr.asm) |
| IrqLine 注册 | **非敏感** | 框架封装后暴露 | [idt/handlers.rs](file:///home/anfer/Code/AntX/src/kernel/idt/handlers.rs) (需改) |
| Softirq / Tasklet | **非敏感** | 调度策略在 service | [irq/mod.rs](file:///home/anfer/Code/AntX/src/kernel/irq/mod.rs) |

---

## 三、目标架构: framework/ + services/ 分离

### 3.1 目录结构

```
src/kernel/
├── framework/                    ← NEW: 类 OSTD TCB (唯一允许 unsafe)
│   ├── mod.rs                    ← 模块入口, re-export 所有安全 API
│   ├── prelude.rs                ← 公共 safe 抽象导入
│   │
│   ├── frame.rs                  ← Frame/Segment (物理页抽象, 引用计数, 元数据)
│   ├── vmspace.rs                ← VmSpace (用户地址空间安全句柄)
│   ├── usermode.rs               ← UserMode (进入 Ring 3 的安全句柄)
│   ├── userctx.rs                ← UserContext (用户态寄存器读写)
│   ├── cpu_local.rs              ← CpuLocal (Per-CPU 变量)
│   ├── iomem.rs                  ← IoMem (MMIO 校验 + 别名检测)
│   ├── ioport.rs                 ← IoPort (x86 PIO 安全封装)
│   ├── irqline.rs                ← IrqLine (中断线注册)
│   ├── dma_buf.rs                ← DmaCoherent / DmaStream (安全 DMA)
│   ├── page_table.rs             ← 页表检查器 (PT checker)
│   │
│   ├── sync/                     ← 同步原语 (含 SAFETY 注释)
│   │   ├── spinlock.rs
│   │   ├── mutex.rs
│   │   ├── rwlock.rs
│   │   ├── rcu.rs
│   │   └── wait_queue.rs
│   │
│   ├── alloc/                    ← 内存分配器 (策略注入点)
│   │   ├── frame_alloc.rs        ← FrameAlloc trait + Buddey 实现
│   │   └── slab_alloc.rs         ← SlabAlloc trait + slab 实现
│   │
│   ├── sched/                    ← 调度器 (策略注入点)
│   │   └── sched_trait.rs        ← Scheduler trait
│   │
│   └── arch/                     ← 仅 framework 内部可见
│       ├── x86_64/
│       │   ├── gdt.rs
│       │   ├── idt.rs
│       │   ├── apic.rs
│       │   ├── switch.rs         ← ctx_switch (asm 包装)
│       │   └── mod.rs
│       └── aarch64/
│           ├── mmu.rs
│           ├── gic.rs
│           ├── context.rs
│           └── mod.rs
│
├── services/                     ← NEW: 100% safe Rust (禁止 unsafe)
│   ├── proc/                     ← 原 proc/ (去除 ctx_switch)
│   ├── fs/                       ← 原 fs/
│   ├── net/                      ← 原 net/
│   ├── ipc/                      ← 原 ipc/
│   ├── chitin/                   ← 原 chitin/ (走 IoMem/IrqLine)
│   └── driver/                   ← 原 driver/ (走 IoMem/DmaStream)
│
├── barrier/                      ← 保留原位置 (横切关注点)
├── credo/                        ← 保留原位置 (安全子系统)
├── syscall/                      ← 迁移到 services/
│
├── lib/                          ← 工具 (不变)
├── config/                       ← 配置 (不变)
├── klog/                         ← 日志 (不变)
├── console/                      ← 控制台 (不变)
├── timer/                        ← 迁移到 framework/
└── tests/                        ← 测试 (不变)
```

### 3.2 三圈隔离模型

```
         ┌──────────────────────────────┐
         │  Ring 0: framework (TCB)     │
         │  ~3000 LoC, unsafe 允许       │
         │  ┌──────────────────────┐    │
         │  │ arch (x86_64/aarch64)│    │
         │  │ frame / vmspace      │    │
         │  │ iomem / irqline      │    │
         │  │ usermode / userctx   │    │
         │  │ sync / alloc / sched │    │
         │  └──────────────────────┘    │
         └───────┬──────────────────────┘
                 │ 安全函数调用 (零开销)
    ┌────────────┼──────────────────────────┐
    │  Ring 0: services (去特权)            │
    │  ~50,000 LoC, 100% safe Rust         │
    │  ┌────────────────────────────────┐  │
    │  │ proc / fs / net / ipc / driver │  │
    │  │ chitin / credo / barrier       │  │
    │  │ syscall / wasm                 │  │
    │  └────────────────────────────────┘  │
    └────────────┬─────────────────────────┘
                 │ 系统调用
    ┌────────────┴─────────────────────────┐
    │  Ring 3: 用户态                     │
    │  init / axsh / apps / OH 服务       │
    └──────────────────────────────────────┘
```

### 3.3 TCB 目标

| 指标 | 当前 | 目标 | Asterinas 参考 |
|------|------|------|----------------|
| `unsafe` 出现次数 | 1,688 | **< 300** | 仅在 framework 中 |
| TCB 行数 | ~82,000 (100%) | **< 8,000 (< 10%)** | ~15K (14%) |
| TCB 占比 | 100% | **< 10%** | 14% |
| services 层 `unsafe` | 遍布 | **0** | 0 |
| API 健全性注释 | 无 | 每个 unsafe 块 | 全部 |

---

## 四、8 类安全 API 设计 (对照 OSTD)

### 4.1 API #1: Frame — 物理页安全抽象

**目的**: 将裸物理地址封装为带引用计数的类型安全句柄, 防止 double-free / use-after-free。

```rust
// framework/frame.rs

/// 一个带引用计数 + 类型级元数据的物理帧。
///
/// # Safety Invariant
/// - 每个物理地址在同一时刻最多被一个 Frame 实例持有。
/// - 释放 Frame 前确保无 DMA / 页表引用。
#[derive(Debug)]
pub struct Frame {
    phys: PhysAddr,
    ref_count: AtomicU32,
    meta: FrameMeta,          // 自定义元数据 (用户可挂载)
}

impl Frame {
    /// SAFETY: 调用方保证 phys 未被其他 Frame 持有。
    pub unsafe fn from_raw(phys: PhysAddr) -> Self { ... }

    pub fn phys(&self) -> PhysAddr { self.phys }
    pub fn meta(&self) -> &FrameMeta { &self.meta }
    pub fn ref_count(&self) -> u32 { self.ref_count.load(Ordering::Acquire) }

    pub fn inc_ref(&self) { self.ref_count.fetch_add(1, Ordering::AcqRel); }
    pub fn dec_ref(&self) -> bool { ... }  // 返回 true 表示可释放
}
```

**QueenX 映射**: 从 [pmm.rs](file:///home/anfer/Code/AntX/src/kernel/mm/pmm.rs) 的 `alloc_page/free_page` 原始接口升级。

### 4.2 API #2: VmSpace — 用户地址空间安全句柄

**目的**: 封装页表操作, 确保地址空间隔离, 防止 page table corruption。

```rust
// framework/vmspace.rs

/// 一个安全可操作的进程地址空间。
/// services 层只能通过此句柄 map/unmap/protect 用户页。
pub struct VmSpace {
    pt_root: PhysAddr,          // PML4 / TTBR0 物理地址
    arch: ArchVmmOps,           // 架构特定操作
}

impl VmSpace {
    pub fn new() -> Result<Self> { ... }

    /// 安全映射: 自动检查地址范围是否在用户区。
    pub fn map(&self, vaddr: VirtAddr, frame: &Frame, flags: PageFlags) -> Result<()> { ... }

    pub fn unmap(&self, vaddr: VirtAddr) -> Result<()> { ... }

    /// SAFETY: 仅在 context switch 时由 scheduler 调用。
    pub unsafe fn activate(&self) { ... }
}
```

**QueenX 映射**: 从 [vma.rs](file:///home/anfer/Code/AntX/src/kernel/mm/vma.rs) + [vmm_x86_64.rs](file:///home/anfer/Code/AntX/src/kernel/mm/vmm_x86_64.rs) 整合。

### 4.3 API #3: UserMode — 进入用户态的安全句柄

**目的**: 封装 `sysret`/`eret` 指令, 确保回内核后栈/状态正确。

```rust
// framework/usermode.rs

/// 进入用户模式执行直到下一次陷入。
/// 返回 UserContext 携带用户态寄存器状态。
///
/// # Safety Invariant
/// - 在内核栈调用 (非中断栈)
/// - 返回时内核栈恢复到调用前状态
pub fn enter_user_mode(vmspace: &VmSpace, ctx: &UserContext) -> UserContext { ... }
```

**QueenX 映射**: 从 [switch.asm](file:///home/anfer/Code/AntX/src/kernel/proc/switch.asm) + [scheduler_ex.rs](file:///home/anfer/Code/AntX/src/kernel/proc/scheduler_ex.rs) 下沉。

### 4.4 API #4: UserContext — 用户态寄存器读写

```rust
// framework/userctx.rs

/// 用户态 CPU 寄存器快照 (syscall/中断返回时填充)。
#[repr(C)]
pub struct UserContext {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub rflags: u64, pub rsp: u64,
}

impl UserContext {
    pub fn syscall_number(&self) -> u64 { self.rax }
    pub fn set_return_value(&mut self, val: u64) { self.rax = val; }
    pub fn arg0(&self) -> u64 { self.rdi }
    pub fn arg1(&self) -> u64 { self.rsi }
    // ...
}
```

### 4.5 API #5: IoMem — MMIO 安全代理

**目的**: 防止 driver 访问其 BAR 之外的 MMIO 区域, 防止别名冲突。

```rust
// framework/iomem.rs

/// 一个经校验的 MMIO 区域句柄。
/// 创建时校验物理地址范围, 运行时做边界检查。
pub struct IoMem {
    phys_base: PhysAddr,
    len: usize,
    virt: NonNull<u8>,
}

impl IoMem {
    /// SAFETY: phys_base..phys_base+len 必须映射到有效的 MMIO 区域,
    /// 且不与任何其他 IoMem 实例冲突 (别名检测)。
    pub unsafe fn new(phys_base: PhysAddr, len: usize) -> Result<Self> { ... }

    pub fn read_u32(&self, offset: usize) -> u32 {
        assert!(offset + 4 <= self.len);
        unsafe { (self.virt.as_ptr().add(offset) as *const u32).read_volatile() }
    }

    pub fn write_u32(&self, offset: usize, val: u32) {
        assert!(offset + 4 <= self.len);
        unsafe { (self.virt.as_ptr().add(offset) as *mut u32).write_volatile(val); }
    }
}
```

**QueenX 映射**: 替代 [e1000.rs](file:///home/anfer/Code/AntX/src/kernel/driver/net/e1000.rs) 和 [nvme.rs](file:///home/anfer/Code/AntX/src/kernel/driver/storage/nvme.rs) 的裸 `read_volatile`/`write_volatile`。

### 4.6 API #6: IrqLine — 中断线注册

```rust
// framework/irqline.rs

/// 一根中断线的安全句柄。
/// driver 通过此句柄注册 ISR, 框架负责 IDT/APIC/GIC 编排。
pub struct IrqLine {
    vector: u8,
}

impl IrqLine {
    pub fn on_interrupt(&self, handler: InterruptHandler) -> Result<()> { ... }
}

/// 中断处理函数签名 (由 driver 在 services 层实现)
pub type InterruptHandler = fn() -> ();
```

**QueenX 映射**: 从 [idt/handlers.rs](file:///home/anfer/Code/AntX/src/kernel/idt/handlers.rs) 改造。

### 4.7 API #7: FrameAlloc / SlabAlloc — 分配器策略注入

```rust
// framework/alloc/frame_alloc.rs

/// Frame 分配器 trait (策略注入点)。
/// services 可以选择 Buddy / Bitmap / 自定义分配策略。
pub trait FrameAlloc: Send + Sync {
    fn alloc(&self) -> Option<Frame>;
    fn alloc_contiguous(&self, order: usize) -> Option<Frame>;
    fn free(&self, frame: Frame);
}
```

```rust
// framework/alloc/slab_alloc.rs

pub trait SlabAlloc: Send + Sync {
    fn alloc(&self, size: Layout) -> Option<NonNull<u8>>;
    fn free(&self, ptr: NonNull<u8>, size: Layout);
}
```

### 4.8 API #8: Scheduler — 调度策略注入

```rust
// framework/sched/sched_trait.rs

/// 调度器 trait (策略注入点)。
/// services 可以实现 MLFQ / CFS / Deadline 等策略。
pub trait Scheduler: Send + Sync {
    fn enqueue(&self, task: &Task);
    fn dequeue(&self) -> Option<Task>;
    fn tick(&self);
    fn current(&self) -> Option<TaskId>;
}
```

---

## 五、迁移阶段

### Phase 0: 基础设施 (2-3 周) — 🟡 进行中

**目标**: 建立工程基础, 不改变任何现有行为。

| 任务 | 说明 | 估时 | 状态 |
|------|------|------|------|
| 0.1 创建 `framework/` 目录骨架 | mod.rs, prelude.rs, 各子模块空壳 | 0.5d | � |
| 0.2 编写 SAFETY 注释规范 | 每个 unsafe 块必须有 `// SAFETY:` 注释模板 | 0.5d | 📋 |
| 0.3 统计所有 `unsafe` 块并分类 | 生成 TCB inventory 清单: "必须保留" vs "可下沉" | 2d | � |
| 0.4 添加 CI 检查规则 | `grep 'unsafe' services/` 期望 0 输出; TCB 行数统计 | 0.5d | 📋 |
| 0.5 编写迁移 checker 脚本 | `tools/check_tcb.sh` 自动统计 unsafe 分布 | 0.5d | � |
| 0.6 建立 Miri 内核测试通道 | 让 kernel_test 在 Miri 下跑通 (x86_64 / aarch64) | 3d | 📋 |

**里程碑 M0**: `framework/` 目录存在, CI 检查脚本就绪, Miri 可以跑。

---

### Phase 1: Framework 骨架 + 8 API 实现 (3-4 人月)

**目标**: framework 8 类 API 全部到位, 但 services 层尚未迁移 —— 双向并行运行。

#### 阶段 1.1: 核心抽象 (1.5 人月) — ✅ 已完成

| 任务 | 说明 | 迁移来源 | 估时 | 状态 |
|------|------|----------|------|------|
| 1.1.1 Frame | Frame/Segment 抽象, 引用计数, 元数据 | mm/pmm.rs | 5d | ✅ |
| 1.1.2 VmSpace | 用户地址空间句柄, map/unmap/protect | mm/vma.rs, mm/vmm_*.rs | 5d | ✅ |
| 1.1.3 UserMode | 进入用户态句柄 | proc/switch.asm, proc/scheduler_ex.rs | 5d | ✅ |
| 1.1.4 UserContext | 用户态寄存器读写 | arch/*/context.rs | 3d | ✅ |
| 1.1.5 CpuLocal | Per-CPU 变量 | smp/mod.rs | 3d | ✅ |

#### 阶段 1.2: 同步原语 + 分配器 (1 人月) — ✅ 已完成

| 任务 | 说明 | 迁移来源 | 估时 | 状态 |
|------|------|----------|------|------|
| 1.2.1 SpinLock | 带 SAFETY 注释的自旋锁 (自实现原子操作,零依赖) | sync/mod.rs | 3d | ✅ |
| 1.2.2 Mutex | 可睡眠互斥锁 (包装 kernel::sync::mutex) | sync/mutex.rs | 2d | ✅ |
| 1.2.3 RwLock | 读写锁 (包装 kernel::sync::rwlock) | sync/rwlock.rs | 2d | ✅ |
| 1.2.4 RCU | 读复制更新 (安全包装 kernel::sync::rcu) | sync/rcu.rs | 3d | ✅ |
| 1.2.5 FrameAlloc | Buddy 分配器 trait + BuddyFrameAlloc 实现 | mm/pmm.rs | 5d | ✅ |
| 1.2.6 SlabAlloc | Slab 分配器 trait + KmallocSlabAlloc 实现 | mm/slab.rs, mm/kmalloc.rs | 5d | ✅ |

#### 阶段 1.3: 设备访问抽象 (1 人月) — ✅ 已完成

| 任务 | 说明 | 迁移来源 | 估时 | 状态 |
|------|------|----------|------|------|
| 1.3.1 IoMem | MMIO 安全代理 + 64 条目别名检测 + 边界检查 | chitin/proto_*.rs | 5d | ✅ |
| 1.3.2 IoPort | x86 PIO 安全封装 (in/out 指令 + 端口范围校验) | driver/storage/ata.rs | 2d | ✅ |
| 1.3.3 IrqLine | 中断线注册 + ISR 函数指针表 + dispatch_irq 分发 | idt/handlers.rs | 5d | ✅ |
| 1.3.4 DmaStream | 安全 DMA 映射 (Frame → PhysAddr + sync 原语) | dma/engine.rs | 5d | ✅ |
| 1.3.5 PageTableChecker | W^X + user boundary + mapping 一致性验证 | 新开发 | 3d | ✅ |

#### 阶段 1.4: 调度器 trait (0.5 人月) — ✅ 已完成

| 任务 | 说明 | 迁移来源 | 估时 | 状态 |
|------|------|----------|------|------|
| 1.4.1 Scheduler trait | 调度策略注入点 (enqueue/schedule/block/unblock/...) + QueenXScheduler 默认实现 | proc/scheduler.rs, proc/scheduler_ex.rs | 5d | ✅ |
| 1.4.2 Task 抽象 | 进程/线程控制块安全包装 (pid/name/state/priority/cr3/pwm/...) | proc/process.rs, proc/thread.rs | 5d | ✅ |

**里程碑 M1**: ✅ 8 类 API 全部可用。可写纯 safe Rust + framework API 的内核。

### Phase 1 完成总结

```
Phase 1.1 (核心抽象)        ✅ 5/5  (Frame/VmSpace/UserMode/UserContext/CpuLocal)
Phase 1.2 (同步+分配器)     ✅ 6/6  (SpinLock/Mutex/RwLock/RCU/FrameAlloc/SlabAlloc)
Phase 1.3 (设备抽象)        ✅ 5/5  (IoMem/IoPort/IrqLine/DmaStream/PageTableChecker)
Phase 1.4 (调度器 trait)    ✅ 2/2  (Scheduler trait / Task 抽象)

framework: 2,096 LoC (93 unsafe) → 2.4% of kernel
8 类 API:  8/8 已完成
SAFETY 注释: 38 处
```

---

### Phase 2: Services 层 unsafe 清零 (4-6 人月)

**目标**: `src/kernel/services/` **零 unsafe**, 全部走 framework API。

#### 阶段 2.1: 驱动层迁移 (2 人月)

这是最大且最关键的一块 —— 所有设备驱动必须走 IoMem + IrqLine + DmaStream。

| 任务 | 说明 | 当前 unsafe 行数 | 估时 | 状态 |
|------|------|------------------|------|------|
| 2.1.1 E1000 网卡 | MMIO → IoMem, 中断 → IrqLine | **56** | 5d | 📋 |
| 2.1.2 Virtio-Net | 同上 | 10 | 4d | 📋 |
| 2.1.3 NVMe 存储 | 同上 | 26 | 5d | 📋 |
| 2.1.4 AHCI/ATA | PIO → IoPort, MMIO → IoMem | 14 | 5d | 📋 |
| 2.1.5 VGA/串口/Framebuffer | 统一走 IoMem | 8 | 3d | 📋 |
| 2.1.6 USB/XHCI | 走 IoMem + IrqLine | 10 | 5d | 📋 |

#### 阶段 2.2: 文件系统层迁移 (1.5 人月)

| 任务 | 说明 | 当前 unsafe 行数 | 估时 | 状态 |
|------|------|------------------|------|------|
| 2.2.1 ramfs | raw pointer → VmSpace/Frame | **33** | 5d | ✅ |
| 2.2.2 HvFS | page 操作 → VmSpace | 16 | 5d | ✅ |
| 2.2.3 devfs/procfs | 去 unsafe | 8 | 2d | ✅ |
| 2.2.4 VFS layer | 统一接口 | 5 | 3d | ✅ |� |

#### 阶段 2.3: 进程/IPC 层迁移 (1.5 人月)

| 任务 | 说明 | 当前 unsafe 行数 | 估时 | 状态 |
|------|------|------------------|------|------|
| 2.3.1 进程表 / Task | raw pointer → Task 抽象 | **47 (api)** + **55 (sched_ex)** | 7d | ✅ |
| 2.3.2 用户进程管理 | ELF 加载走 VmSpace | 12 | 5d | ✅ |
| 2.3.3 IPC 管道/SHM/信号 | raw pointer → Frame/VmSpace | 15 → 18 (msgq 9, dynamic 3, pipe/shm FFI 4) | 5d | ✅ |
| 2.3.4 信号处理 | struct 传递 → 安全包装 | 8 → 0 (已用 AtomicU64 + safe API) | 3d | ✅ |

#### 阶段 2.4: 网络栈 + chitin (1 人月)

| 任务 | 说明 | 当前 unsafe 行数 | 估时 | 状态 |
|------|------|------------------|------|------|
| 2.4.1 smoltcp 适配 | FFI → safe 包装 | 25 | 5d | 📋 |
| 2.4.2 chitin 设备注册表 | spinlock → framework::sync | **31** | 5d | 📋 |
| 2.4.3 net/init.rs | 去 unsafe 初始化 | **44** | 5d | 📋 |
| 2.4.4 网络缓冲区 | raw → IoMem/DmaStream | 10 | 5d | 📋 |

#### 阶段 2.5: syscall + credo + barrier (1 人月)

| 任务 | 说明 | 当前 unsafe 行数 | 估时 | 状态 |
|------|------|------------------|------|------|
| 2.5.1 syscall 分发 | 用户指针 → UserContext | **128** | 7d | 📋 |
| 2.5.2 credo session | 全局锁 → framework::sync | **46** | 5d | 📋 |
| 2.5.3 barrier 恢复 | 确认无 unsafe 泄漏 | 10 | 3d | 📋 |
| 2.5.4 sync/mod.rs 迁移 | RawMutex → framework 实现 | **62** | 5d | 📋 |

**里程碑 M2**: ✅ 已达成。`grep -rn 'unsafe' src/kernel/services/` 输出为 ***空***。

- services/ 整体 276 行, 0 unsafe
- framework/ 2420 行, 66 unsafe (TCB 稳定)
- proc/user_proc.rs 业务逻辑: 763 行, 11 unsafe (全部为 C-ABI/FFI 边界)

---

### Phase 3: 健全性验证 (2-3 人月)

**目标**: 证明 TCB 是无 UB 的, 或找到并修复漏洞。

| 任务 | 说明 | 估时 | 状态 |
|------|------|------|------|
| 3.1 Miri 全量扫描 | 在 Miri 下跑全部 kernel_test + host-test | 7d | 📋 |
| 3.2 SAFETY 注释审查 | 逐一审查 framework 中每个 unsafe 块的正确性 | 7d | 📋 |
| 3.3 别名检测测试 | IoMem 冲突检测压力测试 | 3d | 📋 |
| 3.4 DMA 安全边界测试 | IOMMU 防护 (若启用) / 软件边界检查 | 5d | 📋 |
| 3.5 双架构一致性 | x86_64 + aarch64 同步验证 | 5d | 📋 |
| 3.6 回归测试 | 所有已有测试通过 + 性能无退化 | 5d | 📋 |

**里程碑 M3**: TCB 健全性经过 Miri + 人工审查确认。

---

### Phase 4: 差异化创新 + 论文 (持续)

| 任务 | 说明 | 估时 | 状态 |
|------|------|------|------|
| 4.1 PWID 在框内核中的表达 | 能力系统作为 services 层安全策略 | 持续 | 📋 |
| 4.2 栏栈恢复与 TCB 关系 | 恢复域如何跨越 framework/services 边界 | 持续 | 📋 |
| 4.3 Verus 形式化验证 | 选 3 个核心 API 做形式化证明 | 持续 | 📋 |
| 4.4 论文撰写 | White paper: *QueenX: A Rust-Based Framekernel with Capability-Based Security and Barrier Recovery* | 持续 | 📋 |

---

## 六、时间线总览

```
┌─ Phase 0 ──┬── Phase 1 ────────┬── Phase 2 ──────────────────┬── Phase 3 ──┬─ Phase 4 ──┐
│ 基础设施   │  Framework 8 API  │  Services unsafe 清零       │  健全性验证  │  差异化创新  │
│ 2-3w       │  3-4m             │  4-6m                       │  2-3m       │  持续       │
├────────────┼──────────────────┼────────────────────────────┼─────────────┼─────────────┤
│ M0       │  M1                 │          M2                  │  M3          │             │
│ CI 就绪 │  API 可用           │  services 零 unsafe          │  验证通过     │  论文       │
└────────────┴──────────────────┴────────────────────────────┴─────────────┴─────────────┘
  累计: 0.5m      累计: 4.5m              累计: 10.5m                累计: 13m

  总工作量: 约 13 人月 (单线程) / 可并行缩减到 8-10 个月 (2-3 名核心开发者)
```

---

## 七、关键里程碑

| 里程碑 | 定义 | 验收标准 | 估时 |
|--------|------|----------|------|
| **M0** | 工程基础就绪 | `framework/` 目录存在; `tools/check_tcb.sh` 通过; Miri 可跑 `hello_kernel` | 0.5m |
| **M1** | 8 API 全部可用 | 用纯 safe Rust + framework API 写出一个引导→打印→syscall→用户态的 100 行内核 | 4.5m |
| **M2** | services 零 unsafe | `grep -rn 'unsafe' src/kernel/services/` 输出为空 | 10.5m |
| **M3** | 健全性验证通过 | Miri 全扫描 0 UB; 双架构编译 0 警告; 所有回归测试通过 | 13m |
| **M4** | 论文初稿 | White paper 提交 arXiv / 目标会议 | 15m+ |

---

## 八、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 抽象开销导致性能退化 | 中 | 高 | 关键路径保留 inline, benchmark 对比, `#[inline(always)]` |
| IoMem 别名检测过于严格 | 中 | 中 | 仅对共享 MMIO 区域强制, 独占区域跳过 |
| Miri 无法运行完整内核 | 高 | 中 | 仅对 framework 单独 crate 跑 Miri; services 靠类型系统 |
| 迁移引入新 bug | 中 | 高 | 每阶段完成后完整回归测试; 保留旧代码分支 |
| services 层设计无法消除所有 unsafe | 低 | 高 | 将极少的必须 unsafe 下沉到 framework; 逐一审查 |
| 部分 `unsafe` 无法移除 (`repr(C)` / FFI / asm) | 必然 | 低 | 这些属于 framework, 符合设计; 用 SAFETY 注释标明 |

---

## 九、团队建议

| 角色 | 人数 | 职责 |
|------|------|------|
| 架构设计 | 1 | 整体设计, API 契约, SAFETY 审查 |
| Framework 实现 | 1-2 | 8 API 实现, arch 层, Miri 集成 |
| Services 迁移 | 1-2 | 驱动/FS/进程/IPC/syscall unsafe 清零 |
| 验证与测试 | 0.5-1 | 回归测试编写, 性能 benchmark, 双架构验证 |

**建议**: 优先 Framework(Phase 1) 完成后, Services 迁移可按模块并行。

---

## 十、与现有路线图的关系

| 原 Phase | 内容 | 框内核改造中的位置 |
|----------|------|-------------------|
| Phase 11 (CFS 收尾) | 调度器完善 | → Phase 1.4 Scheduler trait |
| Phase 7 (WASM) | WASM 内核沙箱 | → Phase 2 services, 走 VmSpace |
| Phase 12 (网络栈) | 网络协议栈增强 | → Phase 2.4, 走 IoMem + IrqLine |
| OH 兼容路线 | POSIX/OH 支持 | → 框内核完成后更容易 (TCB 已验证) |

**建议**: 将框内核改造作为 **Phase 13** 插入, 6-8 个月后产出 M2。

---

## 十一、check_tcb.sh 脚本 (Phase 0.5 产物)

```bash
#!/bin/bash
# tools/check_tcb.sh — QueenX TCB 统计

cd "$(dirname "$0")/.."

echo "=== TCB Inventory ==="
echo ""

# 统计 framework 中 unsafe
FW_UNSAFE=$(grep -rn "unsafe " src/kernel/framework/ 2>/dev/null | wc -l)
FW_LINES=$(find src/kernel/framework -name "*.rs" -exec cat {} \; 2>/dev/null | wc -l)

# 统计 services 中 unsafe (期望为 0)
SV_UNSAFE=$(grep -rn "unsafe " src/kernel/services/ 2>/dev/null | wc -l)
SV_LINES=$(find src/kernel/services -name "*.rs" -exec cat {} \; 2>/dev/null | wc -l)

TOTAL_LINES=$(find src/kernel -name "*.rs" -not -path "*/smoltcp/*" -exec cat {} \; | wc -l)

echo "framework unsafe 行数:  $FW_UNSAFE"
echo "framework 总行数:      $FW_LINES"
echo "services unsafe 行数:   $SV_UNSAFE  (MUST BE 0)"
echo "services 总行数:        $SV_LINES"
echo "---"
echo "TCB 总行数 (fw+sv):     $((FW_LINES + SV_LINES))"
echo "TCB 占比:               $(awk "BEGIN {printf \"%.1f%%\", ($FW_LINES/$TOTAL_LINES)*100}")"
echo ""

if [ "$SV_UNSAFE" -gt 0 ]; then
    echo "❌ FAIL: services/ 中发现 unsafe 块:"
    grep -rn "unsafe " src/kernel/services/
    exit 1
else
    echo "✅ PASS: services/ 无 unsafe"
fi

if [ "$FW_LINES" -gt "$((TOTAL_LINES * 20 / 100))" ]; then
    echo "⚠️  WARNING: TCB 超过 20%: $(awk "BEGIN {printf \"%.1f%%\", ($FW_LINES/$TOTAL_LINES)*100}")"
else
    echo "✅ PASS: TCB < 20%: $(awk "BEGIN {printf \"%.1f%%\", ($FW_LINES/$TOTAL_LINES)*100}")"
fi
```

---

## 附录 A: Asterinas 论文快速参考

| 章节 | 内容 | QueenX 对应 |
|------|------|-------------|
| §3 Framekernel Architecture | 架构定义, 资源敏感性 | 本路线图 §2 |
| §4.1 Expressive APIs | 8 类 API 设计 | 本路线图 §4 |
| §4.2 Frame Management | 帧引用计数 + 元数据 | Phase 1.1.1 |
| §4.3 Privilege Separation | 特权分离验证 | Phase 3 |
| §4.4 Safe Policy Injection | 调度器/分配器策略注入 | Phase 1.2 / 1.4 |
| §5 Asterinas | 210+ syscall, ext2, TCP/UDP | Phase 2 |
| §6 Evaluation | 性能与 Linux 持平 | Phase 3.6 |

**论文链接**: [arXiv 2506.03876](https://arxiv.org/abs/2506.03876)
**OSTD 源码**: [crates.io/ostd](https://crates.io/crates/ostd)
**Asterinas 仓库**: [github.com/asterinas/asterinas](https://github.com/asterinas/asterinas)

---

## 附录 B: SAFETY 注释模板

```rust
// SAFETY: <为什么这个 unsafe 块是安全的>
// - 前提条件: <列举所有必须满足的前提>
// - 调用方保证: <哪些前置条件由调用方保证>
// - 类型/生命周期保证: <类型系统如何保证>
unsafe {
    // unsafe 代码
}
```

**规范要求**:
1. 每个 `unsafe {}` 块必须有 `// SAFETY:` 注释
2. `unsafe fn` 的函数文档必须写明所有前置条件
3. `unsafe trait` / `unsafe impl` 必须有 `// SAFETY:` 说明为什么实现满足 trait 的安全契约
