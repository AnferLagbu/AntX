# AntX 内核架构设计

> **最后更新**: 2026-05-13 | **版本**: v3.0 (Rust 重写后)

## 一、架构概述

AntX 内核采用**宏内核（Monolithic Kernel）**架构，核心模块已用 **Rust** 重写（约 60%），聚焦**可理解性优先**目标。

### 设计原则

```
P1: 可理解性 > 性能      (每行代码都应知其存在原因)
P2: 实验性 > 兼容性      (不合理则改，不保留历史包袱)
P3: 个人表达 > 行业标准   (按创始人审美组织)
```

### 语言分布

| 语言 | 占比 | 用途 |
|------|------|------|
| **Rust** | ~60% | 核心模块（MM、进程、FS、PWID、Barrier、DMA、同步原语、IPC） |
| **C** | ~30% | lwIP 网络栈、部分驱动 |
| **NASM 汇编** | ~10% | 引导、中断处理、上下文切换 |

### 系统层次结构

```
┌─────────────────────────────────────────────────────────────┐
│                    AntX 用户态 (Ring 3)                       │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐                     │
│  │ antxsh  │  │ 用户程序 │  │ 用户库  │                     │
│  └─────────┘  └─────────┘  └─────────┘                     │
└─────────────────────────────────────────────────────────────┘
                              │
                        系统调用接口 (int 0x80)
                              │
┌─────────────────────────────────────────────────────────────┐
│                  AntX 内核态 (Ring 0)                        │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │进程调度   │  │ 内存管理  │  │ VFS层    │                │
│  │MLFQ+RT   │  │PMM+VMM   │  ├─RamFS   │                │
│  │ (Rust)   │  │ (Rust)   │  ├─DiskFS  │                │
│  └──────────┘  │kmalloc   │  ├─HvFS    │                │
│                │ (Rust)   │  ├─DevFS   │                │
│                └──────────┘  └─ProcFS  │                │
│                              (全部Rust)  │                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ PWID     │  │ 中断处理  │  │SmartMount│                │
│  │ (Rust)   │  │ (Rust)   │  │ (Rust)   │                │
│  └──────────┘  └──────────┘  └──────────┘                │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ 网络栈    │  │ DMA引擎  │  │ 基础驱动  │                │
│  │lwIP+E1000│  │ (Rust)   │  │ATA/键盘/ │                │
│  │ (C+Rust) │  └──────────┘  │串口/PCI   │                │
│  └──────────┘                 └──────────┘                │
│                              (部分Rust)                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ KLog     │  │ IPC(5种) │  │ 同步原语  │                │
│  │ (Rust)   │  │ (Rust)   │  │ (Rust)   │                │
│  └──────────┘  └──────────┘  └──────────┘                │
└─────────────────────────────────────────────────────────────┘
```

## 二、核心模块（Rust 重写后实现状态）

### 2.1 进程管理模块 (Rust)

**已实现功能**:
- 进程创建、退出、等待 (process_create/exit/wait)
- MLFQ 多级反馈队列调度器 + 实时任务支持 (FIFO/RR)
- 线程模型 (thread_create/block/unblock)
- 多级权限支持 (PWID集成)
- 用户态进程加载与执行 (ELF 加载器, iretq 切换)
- 系统调用分发机制 (int 0x80 + syscall_dispatch)

**关键文件**: 
- `src/kernel/proc/process.rs` - 进程管理
- `src/kernel/proc/scheduler.rs` - MLFQ 调度器
- `src/kernel/proc/thread.rs` - 线程管理
- `src/kernel/proc/session.rs` - 会话管理
- `src/kernel/proc/user_proc.rs` - 用户进程

### 2.2 内存管理模块 (Rust)

**已实现功能**:
- 物理内存管理 (PMM) - 位图分配器
- 虚拟内存管理 (VMM) - 四级页表
- 内核堆 - kmalloc/kfree
- Slab 分配器
- 大页支持 (2MB/1GB)
- 双映射启动 (恒等映射 + 高地址映射)

**关键文件**: 
- `src/kernel/mm/pmm.rs` - 物理内存管理
- `src/kernel/mm/vmm.rs` - 虚拟内存管理
- `src/kernel/mm/kmalloc.rs` - 内核堆
- `src/kernel/mm/slab.rs` - Slab 分配器

### 2.3 文件系统模块 (全部 Rust)

**已实现功能**:
- **VFS 层**: 统一文件操作接口 (`vfs_open/close/read/write/mkdir/mount/unmount`)
- **HvFS**: 原生文件系统，支持:
  - 目录和文件节点
  - Inode 管理
  - 块缓存 (LRU)
  - 位图管理 (Block/Inode bitmap)
  - 磁盘持久化 (ATA PIO)
  - Sync机制 (脏页追踪)
- **RamFS**: 内存文件系统
- **DiskFS**: HvFS磁盘封装
- **DevFS**: 设备文件系统
- **ProcFS**: 进程信息文件系统

**关键文件**:
- `src/kernel/fs/vfs/vfs.rs` - VFS核心
- `src/kernel/fs/hvfs/hvfs.rs` - HvFS核心
- `src/kernel/fs/ramfs/ramfs.rs` - RamFS实现
- `src/kernel/fs/diskfs/diskfs.rs` - DiskFS封装
- `src/kernel/fs/devfs/devfs.rs` - DevFS实现
- `src/kernel/fs/procfs/procfs.rs` - ProcFS实现

### 2.4 网络子系统 (C + Rust)

**已实现**:
- lwIP 2.2.1 完整 TCP/IP 协议栈集成 (C)
- Intel 82540EM (e1000) 网卡驱动 (Rust)
- DHCP 自动获取 IP
- ICMP Echo/Ping
- DNS 解析
- HTTP Server/Client
- TCP/UDP PCB 管理

**关键文件**: 
- `src/kernel/net/driver/e1000.rs` - E1000 驱动 (Rust)
- `src/kernel/net/lwip/` - lwIP 协议栈 (C)
- `src/kernel/net/netif.rs` - 网络接口 (Rust)

### 2.5 DMA 引擎 (Rust)

纯 Rust 实现，提供一致性 DMA、流式 DMA、MMIO 映射 (ioremap)、Scatter-Gather 支持。

**关键文件**: 
- `src/kernel/dma/mod.rs`
- `src/kernel/dma/engine.rs`
- `src/kernel/dma/ffi.rs`

### 2.6 PWID 权限系统 (Rust)

**已实现**:
- PWID 生成/验证 (SHA-256)
- v4 能力掩码模型 — 每个 PWID 携带 16×64 位能力
- First Token（创世令牌）— 首次启动自动生成
- 令牌系统 (token_create/use/revoke/expire)
- 信任链 (trust_add/remove) — 最多 8 跳
- 审计日志
- 能力矩阵 (capability_mask: [u64; 16])

**关键文件**:
- `src/kernel/pwid/mod.rs`
- `src/kernel/pwid/manager.rs`
- `src/kernel/pwid/capability.rs`
- `src/kernel/pwid/token.rs`
- `src/kernel/pwid/trust_chain.rs`

### 2.7 IPC 子系统 (Rust)

5种IPC全部实现: 管道/Pipe、信号/Signal、共享内存/SHM、消息队列/MsgQ、信号量/Semaphore。

**关键文件**:
- `src/kernel/ipc/pipe.rs`
- `src/kernel/ipc/signal.rs`
- `src/kernel/ipc/shm.rs`
- `src/kernel/ipc/msgq.rs`
- `src/kernel/ipc/sem.rs`

### 2.8 Barrier 栈 (Rust)

宏内核故障恢复机制，提供增量回滚、循环防护。

**关键文件**: `src/kernel/barrier/mod.rs`

### 2.9 同步原语 (Rust)

| 原语 | 状态 | 文件 |
|------|------|------|
| Spinlock | ✅ | `src/kernel/sync/spinlock.rs` |
| Atomic | ✅ | `src/kernel/sync/atomic.rs` |
| R/W Lock | ✅ | `src/kernel/sync/rwlock.rs` |
| Mutex | ✅ | `src/kernel/sync/mutex.rs` |

### 2.10 基础驱动

| 驱动 | 状态 | 语言 | 文件 |
|------|------|------|------|
| 串口 (COM1) | ✅ | Rust | `src/kernel/driver/serial.rs` |
| ATA PIO | ✅ | Rust | `src/kernel/driver/ata.rs` |
| PS/2 键盘 | ✅ | Rust | `src/kernel/driver/keyboard.rs` |
| E1000 网卡 | ✅ | Rust | `src/kernel/net/driver/e1000.rs` |
| PIT 定时器 | ✅ | Rust | `src/kernel/timer/pit.rs` |
| PCI 总线 | ✅ | Rust | `src/kernel/pci/mod.rs` |

### 2.11 IDT 中断处理 (Rust)

**关键文件**:
- `src/kernel/idt/idt.rs` - IDT 管理
- `src/kernel/idt/handlers.rs` - 中断处理函数
- `src/kernel/idt/types.rs` - 类型定义

## 三、初始化顺序

```rust
fn kernel_main() {
    // 1. 串口 + KLog (Rust)
    serial_init();
    klog_init();

    // 2. 基础硬件 (Rust + ASM)
    gdt_init();     // Rust
    idt_init();     // Rust
    cpu_init();     // Rust

    // 3. 内存管理 (Rust)
    pmm_init();
    kmalloc_init();
    vmm_init();

    // 4. 进程/会话/调度 (Rust)
    process_init();
    session_init();
    scheduler_init();
    user_proc_init();

    // 5. PWID 权限 (Rust)
    pwid_init();

    // 6. 驱动和文件系统 (Rust)
    ata_init();
    vfs_init();
    ramfs_init();
    hvfs_init();
    diskfs_init();
    devfs_init();
    procfs_init();

    // 7. 系统调用 + 外设 (Rust)
    syscall_init();
    keyboard_init();
    timer_init();

    // 8. 开中断
    enable_interrupts();

    // 9. DMA + 网络栈 (Rust + C)
    dma_init();
    net_init();

    // 10. 用户态
    start_user_init();

    // 11. 空闲循环
    loop {
        e1000_poll();
        interrupt_idle();
    }
}
```

## 四、代码量统计

| 模块 | 文件数 | 估计行数 | 语言 | 状态 |
|------|--------|----------|------|------|
| 内存管理 | ~5 | ~2,400 | Rust | ✅ |
| 进程调度 | ~8 | ~2,000 | Rust | ✅ |
| 文件系统 | ~15 | ~3,000 | Rust | ✅ |
| PWID | ~12 | ~2,500 | Rust | ✅ |
| Barrier | ~1 | ~620 | Rust | ✅ |
| DMA | ~3 | ~500 | Rust | ✅ |
| IPC | ~10 | ~1,000 | Rust | ✅ |
| 同步原语 | ~6 | ~800 | Rust | ✅ |
| IDT | ~6 | ~1,000 | Rust | ✅ |
| KLog | ~1 | ~500 | Rust | ✅ |
| 驱动 | ~6 | ~2,000 | Rust | ✅ |
| 网络栈 (lwIP) | ~100 | ~50,000 | C | ✅ (第三方) |
| 引导/中断入口 | ~3 | ~500 | ASM | ✅ |
| **总计** | **~176** | **~67,000 (含lwIP) / ~17,000 (自研)** | **Rust ~60%** | **目标自研<50K** |

## 五、与其他OS对比

| 维度 | Linux | AntX |
|------|-------|------|
| 架构 | 宏内核 (30M行) | 宏内核 (<50K行自研) |
| 语言 | C + 汇编 | Rust (核心) + C (lwIP) + 汇编 |
| 权限 | UID/GID | PWID (能力掩码) |
| 调度 | CFS | MLFQ + RT |
| FS | VFS + ext4/btrfs... | VFS + HvFS/RamFS/DiskFS/DevFS/ProcFS |
| 网络 | 内核协议栈 | lwIP 2.2.1 |
| 故障恢复 | panic | Barrier 栈 (字段级回滚) |
| 目标 | 通用服务器/桌面 | 个人学习探索 |

---
**文档维护者**: AI Assistant
**创建日期**: 2026-05-06
**最后更新**: 2026-05-13
**基于规范**: ai-autonomous-development-spec.md v2.0
