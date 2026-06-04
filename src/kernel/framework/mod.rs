//! QueenX Framekernel — 特权 OS Framework (TCB)
//!
//! 这是整个内核中**唯一**允许包含 `unsafe` Rust 代码的模块。
//! 所有低层硬件交互 (MMU/DMA/中断/上下文切换) 在此封装为
//! 安全 API, 供 `services/` 层在纯 safe Rust 中调用。
//!
//! ## 架构 (框内核)
//!
//! ```text
//! framework/ (TCB, ~3000 LoC, unsafe 允许)
//!   ├── frame.rs        Frame/Segment 物理页抽象           (✅ 1.1)
//!   ├── vmspace.rs      VmSpace 用户地址空间句柄            (✅ 1.1)
//!   ├── usermode.rs     UserMode 进入 Ring 3 / EL0 句柄    (✅ 1.1)
//!   ├── userctx.rs      UserContext 用户态寄存器            (✅ 1.1)
//!   ├── cpu_local.rs    CpuLocal Per-CPU 变量               (✅ 1.1)
//!   ├── iomem.rs        IoMem MMIO 安全代理                 (✅ 1.3)
//!   ├── ioport.rs       IoPort x86 PIO 封装                 (✅ 1.3)
//!   ├── irqline.rs      IrqLine 中断线注册                  (✅ 1.3)
//!   ├── dma_buf.rs      DmaStream 安全 DMA                  (✅ 1.3)
//!   ├── page_table.rs   PageTableChecker                    (✅ 1.3)
//!   ├── sync/           同步原语                             (✅ 1.2)
//!   ├── alloc/          分配器特质                           (✅ 1.2)
//!   ├── sched/          调度器特质 (TODO 1.4)
//!   └── arch/           架构特定 (仅 framework 内部可见)
//!       ├── x86_64/     GDT/IDT/APIC/ctx_switch
//!       └── aarch64/    MMU/GIC/context/psci
//!
//! services/ (去特权, 100% safe Rust, 禁止 unsafe)
//!   ├── proc/ fs/ net/ ipc/ chitin/ driver/
//!   ├── barrier/ credo/ syscall/ wasm/
//!   └── ...
//! ```
//!
//! ## SAFETY 规范
//!
//! 本模块中每个 `unsafe` 块必须有 `// SAFETY:` 注释, 说明:
//! 1. 前提条件: 哪些不变量在本块内被假设成立
//! 2. 调用方保证: 哪些条件由调用上下文的类型/生命周期保证
//! 3. 硬件契约: 对 CPU/MMU/DMA 行为的假设
//!
//! 参考: `docs/development/api-rs-guideline.md` §附录 B

pub mod frame;
pub mod vmspace;
pub mod usermode;
pub mod userctx;
pub mod userptr;
pub mod cpu_local;

pub mod sync;
pub mod alloc;
pub mod sched;
pub(crate) mod arch;

// Phase 1.3 — 设备访问抽象
pub mod iomem;
pub mod ioport;
pub mod irqline;
pub mod dma_buf;
pub mod page_table;

pub mod net_socket;

pub mod credo_pwm;

pub mod proc_elf;

pub mod syscall_init;

pub mod prelude;

pub mod racy_cell;
