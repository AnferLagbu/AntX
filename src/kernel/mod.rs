//! QueenX 内核 (纯 Rust 实现)
//!
//! ## 架构概览
//!
//! ```text
//! kernel/
//! ├── arch/          # 架构相关 (GDT, TSS, x86_64 特定)
//! │   └── x86_64/
//! ├── boot/          # 启动信息 (Multiboot, 内存映射)
//! ├── cpu/           # CPU 管理 (CPUID, MSR, TSC, 缓存, 拓扑)
//! ├── lib/           # 基础库 (字符串/内存操作, C 标准库函数)
//! ├── mm/            # 内存管理 (PMM, VMM, Slab, Kmalloc)
//! ├── proc/          # 进程/线程管理 (PCB, 调度器, 用户进程)
//! ├── fs/            # 文件系统 (VFS, ramfs, HvFS v2, devfs, procfs)
//! ├── net/           # 网络协议栈 (lwIP, sys_arch, 驱动)
//! ├── idt/           # 中断描述符表 (IDT, handlers, 统计)
//! ├── sync/          # 同步原语 (spinlock, mutex, rwlock)
//! ├── pwm/          # 安全框架 (能力矩阵, 令牌, 信任链)
//! ├── dma/           # DMA 引擎
//! ├── barrier/       # 故障恢复系统
//! ├── pci/           # PCI 设备管理
//! ├── syscall/       # 系统调用接口
//! ├── driver/        # 设备驱动 (ATA, 键盘, 串口)
//! ├── ipc/           # 进程间通信
//! └── timer/         # 定时器子系统
//! ```
//!
//! ## 设计理念
//!
//! - **功能复刻**: 理解 C 版本逻辑后用 Rust 惯用方式重写
//! - **类型安全**: 利用枚举、Option、Result 消除不安全代码
//! - **零成本抽象**: 关键路径性能与 C 版本相当
//! - **模块化**: 每个子系统独立可测试

// ============================================================================
// 核心子系统声明
// ============================================================================

/// 架构相关模块 (GDT, TSS)
pub mod arch;

/// 启动信息模块 (Multiboot, 内存映射)
pub mod boot;

/// CPU 驱动核心 (CPUID, MSR, TSC, 缓存检测, 多核拓扑)
pub mod cpu;

// ============================================================================
// 主要子系统 (从 src/ 根目录提升至此)
// ============================================================================

/// 内存管理子系统 (PMM, VMM, Slab, Kmalloc)
pub mod mm;

/// 进程/线程管理 (PCB, 调度器, MLFQ, 用户进程加载)
pub mod proc;

/// 文件系统 (VFS, ramfs, devfs, procfs, diskfs)
pub mod fs;

/// 网络协议栈 (lwIP, OS 抽象层, 网卡驱动)
/// x86_64: E1000 PCI  /  aarch64: virtio-net MMIO
pub mod net;

/// 中断描述符表 (IDT, ISR, 异常处理, 统计)
pub mod idt;

/// 中断底部半 (Softirq, 延迟处理)
pub mod irq;

/// 同步原语 (SpinLock, Mutex, RwLock, 原子操作)
pub mod sync;

/// PWM v4 安全框架 (能力矩阵, 令牌, 信任链, 审计)
pub mod pwm;

/// DMA 引擎 (映射, 缓冲区管理, 一致性)
pub mod dma;

/// 故障恢复屏障 (panic 恢复, 域隔离)
pub mod barrier;

/// PCI 设备管理 (枚举, 配置空间访问, 双架构 ECAM/Port I/O)
pub mod pci;

/// 系统调用接口 (syscall 表, 参数验证, 双架构支持)
pub mod syscall;

/// 设备驱动 (ATA 磁盘, 键盘, 串口)
pub mod driver;

/// 几丁质设备框架 (Chitin: 统一设备注册/发现/分类)
pub mod chitin;

/// IPC 子系统 (管道, 共享内存, 消息队列, 信号量, 信号)
pub mod ipc;

/// Timer 子系统 (PIT 驱动, Tick 计数器, Sleep 机制)
pub mod timer;

/// 基础库 (字符串/内存操作, C 标准库函数的 Rust 实现)
pub mod lib;

/// 日志系统 (KLog, 多级别, 分类输出)
pub mod klog;
pub mod console;

/// SMP 多核支持 (双架构桩实现, feature=smp 时启用真实 IPI)
pub mod smp;

/// 内核测试框架
pub mod tests;

// ============================================================================
// 重新导出常用类型 (方便其他模块使用)
// ============================================================================

pub use cpu::CpuInfo;
