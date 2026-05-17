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

## 📂 目录结构

```
src/
├── kernel/                 # 内核核心代码
│   ├── boot/              # 启动相关
│   │   ├── boot.asm       # 多引导头、实模式初始化
│   │   ├── entry.asm      # 长模式设置
│   │   └── isr.asm        # 中断服务例程
│   │
│   ├── proc/              # 进程管理
│   │   ├── process.c      # 进程控制块
│   │   ├── scheduler.c    # 调度器
│   │   └── switch.asm     # 上下文切换
│   │
│   ├── mem/               # 内存管理
│   │   ├── pmm.c          # 物理内存管理器
│   │   ├── vmm.c          # 虚拟内存管理器
│   │   └── heap.c         # 堆管理器
│   │
│   ├── fs/                # 文件系统
│   │   ├── vfs/           # 虚拟文件系统
│   │   ├── ramfs/         # 内存文件系统
│   │   ├── hvfs/          # 混合文件系统
│   │   ├── devfs/         # 设备文件系统
│   │   └── procfs/        # 进程文件系统
│   │
│   ├── security/          # 安全子系统
│   │   ├── pwid/          # PWID身份管理
│   │   ├── session/       # 会话管理
│   │   └── audit/         # 审计日志
│   │
│   ├── barrier/           # 栏栈恢复
│   │   ├── snapshot.rs    # 设备快照
│   │   ├── domain.rs      # 恢复域
│   │   └── reset/         # 恢复策略
│   │       ├── bbr.rs     # 基础恢复
│   │       ├── bsr.rs     # 软重启
│   │       └── bhr.rs     # 硬重启
│   │
│   ├── driver/            # 驱动框架
│   │   ├── keyboard.c     # 键盘驱动
│   │   ├── serial.c       # 串口驱动
│   │   └── ata.c          # ATA磁盘驱动
│   │
│   ├── net/               # 网络栈
│   │   ├── lwip/          # LWIP协议栈
│   │   ├── driver/        # 网络驱动
│   │   └── apps/          # 网络应用
│   │
│   ├── ipc/               # 进程间通信
│   │   ├── shm.c          # 共享内存
│   │   └── msgqueue.c     # 消息队列
│   │
│   ├── tests/             # 内核测试
│   │   ├── kernel_test.c  # 测试框架
│   │   └── test_*.c       # 测试用例
│   │
│   └── main.c             # 内核主函数
│
├── rust/                  # Rust内核模块
│   └── src/
│       └── kernel/        # Rust核心实现
│           ├── mem/       # 内存管理
│           ├── proc/      # 进程管理
│           ├── fs/        # 文件系统
│           ├── security/  # 安全子系统
│           ├── barrier/   # 栏栈恢复
│           ├── driver/    # 驱动框架
│           ├── net/       # 网络栈
│           └── tests/     # Rust测试
│
├── user/                  # 用户态程序
│   ├── init/              # init进程
│   ├── axsh/              # Shell
│   └── install/           # 安装向导
│
├── include/               # 头文件
│   ├── kernel.h           # 内核主头文件
│   ├── types.h            # 类型定义
│   └── syscall.h          # 系统调用号
│
└── scripts/               # 构建脚本
    ├── generate_version.sh
    └── gen_embed.py
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
