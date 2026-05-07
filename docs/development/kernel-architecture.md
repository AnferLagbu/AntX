# AntX 内核架构设计

> **最后更新**: 2026-05-07 | **版本**: v2.1 (反映最新实现状态)

## 一、架构概述

AntX 内核采用**宏内核（Monolithic Kernel）**架构，聚焦**可理解性优先**目标。

### 设计原则

```
P1: 可理解性 > 性能      (每行代码都应知其存在原因)
P2: 实验性 > 兼容性      (不合理则改，不保留历史包袱)
P3: 个人表达 > 行业标准   (按创始人审美组织)
```

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
│  └──────────┘  │kmalloc   │  ├─DiskFS  │                │
│                └──────────┘  ├─HvFS    │                │
│                              ├─DevFS   │                │
│  ┌──────────┐  ┌──────────┐  └─ProcFS  │                │
│  │ PWID     │  │ 中断处理  │  ┌──────────┐                │
│  │Token/Trust│  └──────────┘  │SmartMount│                │
│  └──────────┘                 └──────────┘                │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ 网络栈    │  │ DMA引擎  │  │ 基础驱动  │                │
│  │lwIP+E1000│  │ (Rust)   │  │ATA/键盘/ │                │
│  └──────────┘  └──────────┘  │串口/PCI   │                │
│                              └──────────┘                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ KLog     │  │ IPC(5种) │  │ 同步原语  │                │
│  └──────────┘  └──────────┘  └──────────┘                │
└─────────────────────────────────────────────────────────────┘
```

## 二、核心模块（当前实现状态）

### 2.1 进程管理模块

**已实现功能**:
- 进程创建、退出、等待 (process_create/exit/wait)
- MLFQ 多级反馈队列调度器 (Rust重写) + 实时任务支持 (FIFO/RR)
- 线程模型 (thread_create/block/unblock)
- 多级权限支持 (PWID集成)
- 用户态进程加载与执行 (ELF 加载器, iretq 切换)
- 系统调用分发机制 (int 0x80 + syscall_dispatch)

**关键文件**: `src/proc/process.rs`, `src/proc/scheduler.rs`, `src/proc/ffi.rs`

### 2.2 内存管理模块

**已实现功能**:
- 物理内存管理 (PMM) - 位图分配器 (Rust重写)
- 虚拟内存管理 (VMM) - 四级页表 (Rust重写)
- 内核堆 - kmalloc/kfree (Rust实现)
- Slab 分配器 (C实现)
- 大页支持 (2MB/1GB)
- 双映射启动 (恒等映射 + 高地址映射)

**关键文件**: `src/mm/pmm.rs`, `src/mm/vmm.rs`, `src/mm/kmalloc.rs`, `src/kernel/slab.c`

### 2.3 文件系统模块 (VFS + 5后端)

**已实现功能**:
- **VFS 层**: 统一文件操作接口 (`vfs_open/close/read/write/mkdir/mount/unmount`)
- **HvFS**: 原生文件系统 (Rust重写)，支持:
  - 目录和文件节点
  - Inode 管理
  - 块缓存 (LRU)
  - 位图管理 (Block/Inode bitmap)
  - 磁盘持久化 (ATA PIO)
  - Sync机制 (脏页追踪)
- **RamFS**: 内存文件系统 (Rust重写)
- **DiskFS**: HvFS磁盘封装 (Rust重写)
- **DevFS**: 设备文件系统
- **ProcFS**: 进程信息文件系统

**关键文件**:
- `src/fs/vfs/vfs.rs` - VFS核心 (Rust)
- `src/fs/hvfs/hvfs.rs` - HvFS核心
- `src/fs/ramfs/ramfs.rs` - RamFS实现
- `src/fs/diskfs/diskfs.rs` - DiskFS封装

### 2.4 Smart Mount (智能持久化挂载)

**实现位置**: `src/kernel/smart_mount.c`

**三种模式**:

| 模式 | 宏定义 | 行为 |
|------|--------|------|
| DEV (默认) | 无 | RamFS优先，自动检测磁盘 |
| TEST | BUILD_TEST | 环境变量控制 |
| RELEASE | BUILD_RELEASE | 强制磁盘，失败panic |

**关键函数**: `smart_mount_root()`, `get_persistent_mode()`

### 2.5 网络子系统

**已实现**:
- lwIP 2.2.1 完整 TCP/IP 协议栈集成
- Intel 82540EM (e1000) 网卡驱动
- DHCP 自动获取 IP
- ICMP Echo/Ping
- DNS 解析
- HTTP Server/Client
- mDNS/MQTT/NetBIOS/SMTP/SNMP/SNTP/TFTP 应用
- TCP/UDP PCB 管理

**关键文件**: `src/net/`, `src/net/driver/e1000.c`

### 2.6 DMA 引擎

纯 Rust 实现，提供一致性 DMA、流式 DMA、MMIO 映射 (ioremap)、Scatter-Gather 支持。

**关键文件**: `src/dma/mod.rs`, `src/dma/engine.rs`

### 2.7 PWID 权限系统

**已实现**:
- PWID 生成/验证 (SHA-256)
- 三级权限 (Root/Trustworthy/Untrustworthy)
- 原 Root 锚点 (不可删除)
- 令牌提权 (token_create/use/revoke)
- 信任链 (trust_add/remove)
- 暴力破解防护
- 审计日志
- 能力矩阵 (capability)
- PWID 过期

### 2.8 IPC 子系统

5种IPC全部实现: 管道/Pipe、信号/Signal、共享内存/SHM、消息队列/MsgQ、信号量/Semaphore。

**关键文件**: `src/ipc/ipc.c`

### 2.9 基础驱动

| 驱动 | 状态 | 说明 |
|------|------|------|
| 串口 (COM1) | ✅ | 调试输出 + 输入 |
| ATA PIO | ✅ | 磁盘读写 |
| PS/2 键盘 | ✅ | 用户输入 |
| E1000 网卡 | ✅ | Intel 82540EM |
| PIT 定时器 | ✅ | 100Hz |
| PCI 总线 | ✅ | 设备扫描 |

### 2.10 同步原语

| 原语 | 状态 | 文件 |
|------|------|------|
| Spinlock | ✅ | `src/kernel/spinlock.c` |
| Atomic | ✅ | `src/kernel/atomic.c` |
| R/W Lock | ✅ | `src/kernel/rwlock.c` |
| Mutex | ✅ | `src/kernel/mutex.c` |

## 三、初始化顺序 (main.c)

```c
void kernel_main(void) {
    // 1. 串口 + KLog
    serial_init();  klog_init();

    // 2. 基础硬件
    gdt_init();     idt_init();         cpu_init();

    // 3. 内存管理
    pmm_init();     kmalloc_init();     pmm_init_bitmap();  vmm_init();

    // 4. 进程/会话/调度 (Rust)
    process_init(); session_init();     scheduler_init();
    kernel_init();  user_proc_init();

    // 5. PWID 权限
    pwid_init();

    // 6. 驱动和文件系统
    ata_init();     hvfs_init();        vfs_init();
    ramfs_init();   diskfs_init();      devfs_init();       procfs_init();

    // 7. Smart Mount
    smart_mount_root();

    // 8. 系统调用 + 外设
    syscall_init(); keyboard_init();    timer_init();

    // 9. PWID 恢复
    pwid_try_load();

    // 10. 模块版本注册
    version_register("QueenX", ...); // +10个模块

    // 11. 开中断
    enable_interrupts();

    // 12. DMA + 网络栈
    dma_init();     qx_net_init();

    // 13. 用户态
    start_user_init();

    // 14. 空闲循环
    while (1) { e1000_poll(); interrupt_idle(); }
}
```

## 四、代码量统计

| 模块 | 文件数 | 估计行数 | 状态 |
|------|--------|----------|------|
| 内核核心 | ~15 | ~4000 | ✅ |
| 内存管理 (Rust) | ~4 | ~2400 | ✅ 已完成Rust重写 |
| 进程调度 (Rust) | ~8 | ~2000 | ✅ 已完成Rust重写 (MLFQ+RT) |
| 文件系统 (Rust) | ~10 | ~3000 | ✅ 已完成Rust重写 |
| PWID (Rust) | ~10 | ~2500 | ✅ 已完成Rust重写 |
| DMA (Rust) | ~3 | ~500 | ✅ 已完成Rust重写 |
| 驱动 | ~6 | ~3000 | ✅ |
| IPC | ~1 | ~600 | ✅ |
| 网络栈 (lwIP) | ~100 | ~50000 | ✅ (引入第三方) |
| 用户程序 | ~5 | ~800 | ✅ |
| **总计** | **~160** | **~70000 (含lwIP) / ~20000 (自研)** | **目标自研<50K** |

## 五、与其他OS对比

| 维度 | Linux | AntX |
|------|-------|------|
| 架构 | 宏内核 (30M行) | 宏内核 (<50K行自研) |
| 语言 | C + 汇编 | C + Rust (核心模块Rust) |
| 权限 | UID/GID | PWID (密码+备注) |
| 调度 | CFS | MLFQ + RT |
| FS | VFS + ext4/btrfs... | VFS + HvFS/RamFS/DiskFS/DevFS/ProcFS |
| 挂载 | mount(2) syscall | smart_mount (3模式) |
| 网络 | 内核协议栈 | lwIP 2.2.1 |
| 目标 | 通用服务器/桌面 | 个人学习探索 |

---
**文档维护者**: AI Assistant
**创建日期**: 2026-05-06
**最后更新**: 2026-05-07
**基于规范**: ai-autonomous-development-spec.md v2.0
