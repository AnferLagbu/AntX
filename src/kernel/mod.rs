//! QueenX 内核 (纯 Rust 实现) — 框内核 (Framekernel)
//!
//! ## 架构概览 (Asterinas OSTD 范式)
//!
//! ```text
//! kernel/
//! ├── framework/   # 【唯一 TCB / 唯一允许 unsafe】底层硬件基座
//! │   ├── arch/      架构特定 (GDT/IDT/APIC/MMU/GIC)
//! │   ├── boot/      引导协议 (Multiboot2/UEFI/...)
//! │   ├── cpu/       CPU 探测 (CPUID/MSR/TSC/缓存/拓扑)
//! │   ├── mm/        物理/虚拟内存 (PMM/VMM/Slab/Kmalloc)
//! │   ├── irq/       中断控制器底层
//! │   ├── idt/       中断描述符表
//! │   ├── dma/       DMA 引擎
//! │   ├── driver/    原生硬件驱动 (寄存器/时序)
//! │   ├── net/       网络硬件 + 协议栈
//! │   ├── fs/        文件系统底层 (VFS 抽象 + 块设备层)
//! │   ├── ipc/       IPC 底层 (内核态通道)
//! │   ├── credo/     身份/密码学硬件
//! │   ├── chitin/    设备框架底层
//! │   ├── barrier/   弹性恢复底层
//! │   ├── console/   串口/终端硬件
//! │   ├── klog/      日志硬件输出
//! │   ├── config/    硬件相关配置
//! │   ├── smp/       多核支持
//! │   ├── lib/       底层工具
//! │   ├── link/      链接脚本
//! │   ├── alloc/     全局分配器
//! │   ├── sync/      同步原语 TCB (11 子模块: spinlock/mutex/rwlock/rcu/atomic/seqlock/types/arch/once_lock/once_cell/irq_spinlock)
//! │   ├── proc/      进程管理 TCB (12 子模块: types/process/thread/session/elf/api/scheduler/scheduler_ex/cfs/cpu_queue/oomd/user_proc)
//! │   ├── sched/     调度器特质
//! │   ├── syscall/   系统调用底层
//! │   ├── timer/     时钟底层
//! │   ├── wasm/      WASM 运行时底层
//! │   ├── pci/       PCI 设备底层
//! │   ├── tests/     框架单元测试
//! │   └── frame.rs/vmspace.rs/usermode.rs/userctx.rs/userptr.rs
//! │     iomem.rs/ioport.rs/irqline.rs/dma_buf.rs/page_table.rs
//! │     cpu_local.rs/racy_cell.rs
//! │     net_socket.rs/credo_pwm.rs/proc_elf.rs/syscall_init.rs
//! │
//! └── services/    # 【全 safe / #![deny(unsafe_code)]】业务层
//!     ├── driver/    设备驱动 safe wrapper
//!     ├── fs/        文件系统业务 (VFS + 4 FS 实现)
//!     ├── net/       网络业务 (socket)
//!     ├── ipc/       IPC 业务
//!     ├── proc/      进程子系统
//!     ├── sync/      同步原语业务封装
//!     ├── syscall/   系统调用分发
//!     ├── credo/     身份/密码学业务
//!     ├── chitin/    用户态驱动框架
//!     ├── barrier/   弹性归因业务
//!     ├── console/   控制台业务 (services 实际通过 framework::console 复用)
//!     ├── klog/      日志业务 (services 实际通过 framework::klog 复用)
//!     └── wasm/      WASM 运行时
//! ```
//!
//! ## 设计理念
//!
//! - **TCB 收拢**: 所有 `unsafe` 与硬件裸操作集中于 `framework/`
//! - **业务隔离**: `services/` 全目录 `#![deny(unsafe_code)]`, 100% safe
//! - **类型安全**: 利用枚举、Option、Result 消除不安全代码
//! - **零成本抽象**: 关键路径性能与 C 版本相当
//! - **模块化**: 每个子系统独立可测试

// ============================================================================
// 顶层声明: 仅 2 个目录
// ============================================================================

/// 框内核 Framework (TCB) — 唯一允许 unsafe 的模块
pub mod framework;

/// Services 层 — 去特权 100% safe Rust (框内核架构)
pub mod services;
