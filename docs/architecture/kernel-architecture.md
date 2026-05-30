# 内核架构

> AntX内核的详细架构设计与模块组织

---

## 🏗️ 架构概览

AntX采用**分层宏内核架构**，在保持宏内核高性能的同时，通过栏栈机制实现模块隔离效果。

```
┌─────────────────────────────────────────────────────────┐
│                      应用层                             │
│         用户程序、Shell、系统服务                       │
└─────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────┐
│                    系统调用层                           │
│         syscall.c - 系统调用分发与参数验证             │
└─────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────┐
│                    核心服务层                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│  │ 进程管理 │  │ 内存管理 │  │ 文件系统 │             │
│  └──────────┘  └──────────┘  └──────────┘             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│  │ 安全子系统│  │ 栏栈恢复 │  │ IPC机制  │             │
│  └──────────┘  └──────────┘  └──────────┘             │
└─────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────┐
│                    驱动框架层                           │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐             │
│  │ 块设备   │  │ 字符设备 │  │ 网络设备 │             │
│  └──────────┘  └──────────┘  └──────────┘             │
└─────────────────────────────────────────────────────────┘
                          ↕
┌─────────────────────────────────────────────────────────┐
│                    硬件抽象层                           │
│         IDT、GDT、PIC、Timer、DMA                      │
└─────────────────────────────────────────────────────────┘
```

---

## 📂 目录结构（Rust 实现）

```
src/
├── kernel/                    # 内核主模块树 (Rust)
│   ├── mod.rs                 # 模块入口: pub mod 声明 + 子系统导出
│   ├── arch/                  # 架构相关
│   │   ├── x86_64/           # GDT, TSS, APIC, IOAPIC, ACPI, SMP, trampoline
│   │   └── aarch64/          # MMU, GIC, PSCI, Timer, UART, Exception
│   ├── boot/                  # 启动信息
│   │   ├── boot.asm          # Multiboot1/2 Header, 实→保护→长模式
│   │   ├── entry.asm         # 64位 entry, GDT 加载
│   │   ├── isr.asm           # ISR 入口存根
│   │   ├── stage1.asm        # Stage1 磁盘引导
│   │   ├── mod.rs            # Multiboot 信息解析 + BootInfo
│   │   └── aarch64/          # AArch64 start.S + entry.rs
│   ├── mm/                    # 内存管理
│   │   ├── pmm.rs            # 物理页分配器 (Buddy Allocator)
│   │   ├── vmm.rs            # 虚拟内存管理 (四级页表)
│   │   ├── vmm_aarch64.rs    # AArch64 页表操作
│   │   ├── vma.rs            # 虚拟内存区域管理
│   │   ├── kmalloc.rs        # 内核堆分配器 (First-Fit)
│   │   ├── kmalloc_slab.rs   # Slab 分配器
│   │   ├── slab.rs           # Slab 缓存
│   │   ├── cow.rs            # COW 引用计数
│   │   ├── page_fault.rs     # 缺页处理
│   │   ├── pressure.rs       # 内存压力感知
│   │   └── ffi.rs            # C FFI 桥接
│   ├── proc/                  # 进程/线程管理
│   │   ├── scheduler.rs      # 主调度器 (per-CPU RunQueue)
│   │   ├── cfs.rs            # CFS 公平调度类
│   │   ├── scheduler_ex.rs   # 实时调度 + OOMD
│   │   ├── elf.rs            # ELF64 加载器
│   │   ├── process.rs        # 进程控制块 + fork/exec/exit
│   │   ├── thread.rs         # 内核线程
│   │   ├── user_proc.rs      # 用户态进程管理
│   │   └── cpu_queue.rs      # Per-CPU 运行队列
│   ├── fs/                    # 文件系统
│   │   ├── vfs/              # 虚拟文件系统 (trait + 统一接口)
│   │   ├── ramfs/            # 内存文件系统
│   │   ├── hvfs/             # HvFS v2 (SPA/DMU/ZAP/TXG/ZIL/ARC/RAIDZ/Snap/Dedup)
│   │   ├── devfs/            # 设备文件系统
│   │   └── procfs/           # 进程文件系统
│   ├── credo/                 # CREDO 身份与权限框架
│   │   ├── identity.rs       # PWID 身份表
│   │   ├── capability.rs     # 16×64 能力矩阵
│   │   ├── session.rs        # 会话管理
│   │   ├── engine.rs         # 权限检查引擎
│   │   ├── grant.rs          # 能力委托
│   │   ├── bootstrap.rs      # 信任根初始化
│   │   ├── audit.rs          # 审计日志
│   │   └── storage.rs        # 持久化存储
│   ├── barrier/               # Barrier 栏栈恢复
│   │   ├── domain.rs         # RecoveryDomain
│   │   ├── manager.rs        # RecoveryManager + 心跳监控
│   │   ├── undo_log.rs       # UndoLog 回滚
│   │   ├── snapshot.rs       # 设备快照
│   │   ├── recoverable.rs    # RecoverableMutex
│   │   ├── recovery.rs       # RecoveryDomain trait + 级联恢复
│   │   └── reset/            # BBR/BSR/BHR 分层恢复
│   ├── driver/                # 设备驱动
│   │   ├── bus/pci.rs        # PCI 枚举
│   │   ├── char/vga.rs       # VGA 文本模式
│   │   ├── char/serial.rs    # 串口
│   │   ├── display/          # 显示驱动 (framebuffer/HDMI/DP/font)
│   │   ├── input/keyboard.rs # PS/2 键盘
│   │   ├── storage/          # ATA/AHCI/NVMe 存储
│   │   ├── virtio/           # virtio-blk/virtio-net/virtqueue
│   │   └── usb/              # xHCI USB
│   ├── net/                   # 网络栈
│   │   ├── lwip/             # lwIP 2.2.1 协议栈 (C)
│   │   ├── driver/e1000.rs   # Intel e1000 网卡
│   │   ├── sys_arch.rs       # lwIP OSAL (Rust)
│   │   ├── netif.rs          # 网络接口管理
│   │   └── apps.rs           # HTTP/mDNS/SNTP 等服务
│   ├── ipc/                   # 进程间通信
│   │   ├── pipe.rs           # 管道
│   │   ├── shm.rs            # 共享内存
│   │   ├── msgq.rs           # 消息队列
│   │   ├── sem.rs            # 信号量
│   │   ├── signal.rs         # 信号
│   │   └── dynamic.rs        # 动态 IPC 命名空间
│   ├── sync/                  # 同步原语
│   │   ├── spinlock.rs       # 自旋锁
│   │   ├── mutex.rs          # 互斥锁
│   │   ├── rwlock.rs         # 读写锁
│   │   ├── seqlock.rs        # 序列锁
│   │   ├── rcu.rs            # RCU
│   │   └── atomic.rs         # 原子操作统计
│   ├── syscall/               # 系统调用接口
│   │   ├── mod.rs            # syscall 分发
│   │   └── types.rs          # 系统调用号 + errno 定义
│   ├── chitin/                # Chitin 设备框架
│   ├── idt/                   # 中断描述符表
│   ├── irq/                   # 中断底部半 (Softirq)
│   ├── timer/                 # 定时器 (PIT + Tick)
│   ├── cpu/                   # CPU 管理 (CPUID/MSR/TSC/拓扑)
│   ├── dma/                   # DMA 引擎
│   ├── console/               # 图形控制台
│   ├── klog/                  # 内核日志
│   ├── smp/                   # SMP 多核
│   ├── wasm/                  # WASM 解释器沙箱
│   ├── link/                  # 链接脚本
│   │   ├── x86_64.ld         # x86_64 链接脚本
│   │   └── aarch64.ld        # aarch64 链接脚本
│   └── tests/                 # 内核内嵌测试
│
├── rust/                      # Rust 内核库 crate
│   └── src/
│       ├── lib.rs             # #![no_std] + #![no_main] + 全局配置
│       └── memory_allocator.rs # 全局分配器 (GlobalAlloc trait)
│
└── user/                      # 用户态程序 workspace
    ├── lib/                   # userlib (syscall 封装)
    ├── init/                  # init 进程
    ├── axsh/                  # Shell
    ├── install/               # 安装向导
    ├── fbterm/                # 帧缓冲终端
    └── httpsrv/               # HTTP 服务器
```

---

## 🔧 核心模块详解

### 1. 启动模块 (boot/)

**职责**: 从BIOS到内核初始化的完整流程

**关键文件**:
- `boot.asm`: Multiboot2头、实模式到保护模式
- `entry.asm`: 设置长模式、初始化GDT
- `isr.asm`: 中断服务例程入口

**流程**:
```
BIOS → GRUB → boot.asm → entry.asm → kernel_main()
```

---

### 2. 进程管理模块 (proc/)

**职责**: 进程生命周期管理、调度、上下文切换

**关键数据结构**:

```c
// 进程控制块
typedef struct {
    uint32_t pid;              // 进程ID
    uint64_t pwid;             // 特权工作负载ID
    process_state_t state;     // 进程状态
    uint8_t priority;          // 优先级
    uint64_t time_slice;       // 时间片
    
    // 内存管理
    uint64_t cr3;              // 页表基址
    uint64_t kernel_stack;     // 内核栈
    uint64_t user_stack;       // 用户栈
    
    // 上下文
    cpu_context_t context;     // CPU上下文
    
    // 文件描述符
    file_descriptor_t fds[MAX_FDS];
    
    // 统计信息
    uint64_t cpu_time;         // CPU时间
    uint64_t start_time;       // 启动时间
} process_t;
```

**调度策略**:
- 优先级调度（0-255，0最高）
- 时间片轮转
- 抢占式调度

---

### 3. 内存管理模块 (mem/)

**职责**: 物理内存、虚拟内存、堆管理

**三层架构**:

```
┌─────────────────────────────────┐
│        堆管理器         │  ← kmalloc/kfree
├─────────────────────────────────┤
│      虚拟内存管理器      │  ← 页表、映射
├─────────────────────────────────┤
│      物理内存管理器      │  ← 物理页分配
└─────────────────────────────────┘
```

**PMM实现**:
- 位图分配算法
- 支持连续页分配
- 支持DMA区域

**VMM实现**:
- 四级页表（PML4→PDPT→PD→PT）
- 写时复制（COW）
- 延迟映射

**堆管理器**:
- 二分伙伴系统
- Slab分配器（小对象）
- 边界标记检测

---

### 4. 文件系统模块 (fs/)

**职责**: 统一文件系统接口、多种文件系统支持

**VFS抽象层**:

```c
// 文件系统操作
struct file_system_ops {
    int (*mount)(const char *path);
    int (*unmount)(const char *path);
    int (*open)(const char *path, int flags);
    int (*close)(int fd);
    ssize_t (*read)(int fd, void *buf, size_t count);
    ssize_t (*write)(int fd, const void *buf, size_t count);
    int (*stat)(const char *path, struct stat *st);
    int (*mkdir)(const char *path);
    int (*unlink)(const char *path);
};
```

**支持的文件系统**:

| 文件系统 | 类型 | 特性 | 用途 |
|---------|------|------|------|
| RamFS | 内存 | 高性能、易失 | 测试、临时文件 |
| HvFS | 混合 | 持久化、快照 | 主文件系统 |
| DevFS | 设备 | 动态设备节点 | 设备访问 |
| ProcFS | 进程 | 进程信息 | 调试、监控 |

---

### 5. 安全子系统 (security/)

**职责**: 身份认证、权限检查、审计

**PWID模型**:

```rust
// PWID结构（128位）
struct Pwid {
    identity: u64,      // 身份标识（60位熵）
    level: u8,          // 特权等级（0=最高）
    flags: u8,          // 标志位
    reserved: u16,      // 保留
}

// 能力矩阵（16×64位）
struct CapabilityMatrix {
    caps: [u64; 16],    // 1024个能力位
}
```

**权限检查流程**:
```
请求 → 解析PWID → 检查特权等级 → 检查能力矩阵 → 允许/拒绝
```

---

### 6. 栏栈恢复模块 (barrier/)

**职责**: 故障检测、恢复策略执行

**三层恢复策略**:

```
┌─────────────────────────────────┐
│  BHR (Barrier Hard Reset)       │  ← 硬件级重置
│  - 禁用中断                      │
│  - 关闭所有设备                  │
│  - 键盘控制器重置                │
├─────────────────────────────────┤
│  BSR (Barrier Soft Reset)       │  ← 软重启
│  - 冻结所有恢复域                │
│  - 回滚到初始栏                  │
│  - 重置设备状态                  │
├─────────────────────────────────┤
│  BBR (Barrier Base Recovery)    │  ← 基础恢复
│  - 定位故障域                    │
│  - 单域回滚                      │
│  - 级联依赖处理                  │
└─────────────────────────────────┘
```

**恢复域**:

```rust
struct RecoveryDomain {
    id: u64,                    // 域ID
    name: &'static str,         // 域名称
    state: DomainState,         // 域状态
    barrier_generation: u64,    // 栏代数
    undo_stack: UndoStack,      // Undo栈
    addr_ranges: Vec<(u64, u64)>, // 地址范围
}
```

---

### 7. 驱动框架模块 (driver/)

**职责**: 统一驱动模型、设备管理

**Driver Trait**:

```rust
pub trait Driver: Send + Sync {
    fn name(&self) -> &'static str;
    fn init(&mut self) -> Result<(), DriverError>;
    fn deinit(&mut self) -> Result<(), DriverError>;
    fn handle_interrupt(&mut self, vector: u8);
    fn device_type(&self) -> DeviceType;
}
```

**设备类型**:
- BlockDevice: 块设备（磁盘）
- CharDevice: 字符设备（键盘、串口）
- NetworkDevice: 网络设备（网卡）

---

### 8. 网络栈模块 (net/)

**职责**: 网络协议、网络服务

**LWIP集成**:
- TCP/UDP协议
- ICMP/ARP协议
- IPv4/IPv6支持
- DHCP客户端

**网络应用**:
- HTTP服务器
- Telnet服务器
- Ping工具

---

## 🔄 模块间交互

### 系统调用流程

```
用户程序
    ↓ int 0x80
syscall_handler()
    ↓ 参数验证
    ├─ 进程系统调用 → proc模块
    ├─ 文件系统调用 → vfs模块
    ├─ 内存系统调用 → mem模块
    ├─ 安全系统调用 → security模块
    └─ 网络系统调用 → net模块
    ↓ 返回结果
用户程序
```

### 文件I/O流程

```
用户程序: read(fd, buf, count)
    ↓ syscall
vfs_read(fd, buf, count)
    ↓ 查找文件描述符
    ↓ 获取inode
    ↓ 检查权限 (security模块)
    ↓ 调用具体文件系统
    ├─ RamFS → ramfs_read()
    ├─ HvFS → hvfs_read()
    ├─ DevFS → devfs_read()
    └─ ProcFS → procfs_read()
    ↓ 返回数据
用户程序
```

### 故障恢复流程

```
硬件异常 (Page Fault, GPF, etc.)
    ↓ IDT处理
    ↓ 栏栈捕获
    ├─ 定位恢复域 (BBR)
    ├─ 检查依赖关系
    ├─ 执行回滚
    │   ├─ Undo栈回放
    │   ├─ 状态恢复
    │   └─ 重新初始化
    ├─ 如失败 → BSR
    └─ 如仍失败 → BHR
    ↓ 恢复完成
继续执行
```

---

## 🎯 设计原则

### 1. 模块化
- 清晰的模块边界
- 最小化模块间耦合
- 明确的接口定义

### 2. 可测试性
- 每个模块可独立测试
- 提供测试桩和模拟
- 高测试覆盖率

### 3. 安全性
- 最小权限原则
- 边界检查
- 输入验证

### 4. 性能
- 避免不必要的拷贝
- 优化关键路径
- 使用高效数据结构

### 5. 可维护性
- 清晰的代码结构
- 完善的注释
- 一致的编码风格

---

## 🔮 未来扩展

### 计划中的模块

1. **虚拟化模块**
   - KVM风格虚拟化
   - 容器支持

2. **安全增强模块**
   - ASLR实现
   - MAC策略

3. **性能监控模块**
   - 性能计数器
   - 追踪框架

4. **电源管理模块**
   - ACPI支持
   - 省电模式

---

**最后更新**: 2026-05-18
