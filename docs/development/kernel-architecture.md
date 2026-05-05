# AntX 内核架构设计

> **最后更新**: 2026-05-06 | **版本**: v2.0 (反映 smart_mount 集成)

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
                        系统调用接口 (int $0x80)
                              │
┌─────────────────────────────────────────────────────────────┐
│                  AntX 内核态 (Ring 0)                        │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ 进程调度  │  │ 内存管理  │  │ VFS层    │                │
│  │          │  │          │  ├─RamFS   │                │
│  └──────────┘  └──────────┘  ├─DiskFS  │                │
│                              └─HvFS    │                │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ PWID     │  │ 中断处理  │  │ 基础驱动  │                │
│  └──────────┘  └──────────┘  └──────────┘                │
│                                                              │
│  ⭐ [新增] Smart Mount (智能持久化挂载)                      │
└─────────────────────────────────────────────────────────────┘
```

## 二、核心模块（当前实现状态）

### 2.1 进程管理模块

**已实现功能**:
- 进程创建、退出、调度 (Round-Robin)
- 多级权限支持 (PWID集成)
- 用户态进程加载与执行
- 系统调用分发机制

**关键文件**: `src/kernel/process.c`, `src/fs/vfs/ffi.rs`

### 2.2 内存管理模块

**已实现功能**:
- 物理内存管理 (PMM) - 页分配
- 虚拟内存管理 (VMM) - 页表映射
- 内核堆 (Heap) - kmalloc/kfree
- 幻数检测 (Magic Number) 防止溢出

**关键文件**: `src/kernel/pmm.c`, `src/kernel/vmm.c`, `src/kernel/heap.c`

### 2.3 文件系统模块 (VFS + HvFS)

**已实现功能**:
- **VFS 层**: 统一文件操作接口 (`vfs_open/close/read/write/mkdir`)
- **HvFS**: 原生文件系统，支持:
  - 目录和文件节点
  - Inode 管理 (128个)
  - 块缓存 (LRU, 1024块)
  - 位图管理 (Block/Inode bitmap)
  - 磁盘持久化 (ATA PIO)
  - Sync机制 (脏页追踪)
- **RamFS**: 内存文件系统 (开发用)
- **DiskFS**: HvFS磁盘封装

**关键文件**: 
- `src/fs/vfs/ffi.rs` - FFI导出层
- `src/fs/hvfs/hvfs.rs` - HvFS核心实现
- `src/fs/ramfs/ramfs.rs` - RamFS实现
- `src/fs/diskfs/diskfs.rs` - DiskFS封装

### 2.4 Smart Mount (智能持久化挂载) [v2.0 新增]

**实现位置**: `src/kernel/smart_mount.c`

**三种模式**:

| 模式 | 宏定义 | 行为 |
|------|--------|------|
| DEV (默认) | 无 | RamFS优先，自动检测磁盘 |
| TEST | BUILD_TEST | 环境变量控制 |
| RELEASE | BUILD_RELEASE | 强制磁盘，失败panic |

**决策流程**:
```
smart_mount_root()
    ↓
detect_persistent_storage()  // 检测 ATA 磁盘
    ↓
[DEV模式] → 尝试 DiskFS → 失败则回退 RamFS
[TEST]   → FORCE_PERSISTENT=1 ? 磁盘 : RamFS
[RELEASE] → 必须使用磁盘 → 否则 panic()
```

**关键函数**:
- `smart_mount_root()` - 主入口
- `get_persistent_mode()` - 返回 'D'/'T'/'R'

### 2.5 基础驱动模块

**已实现**:
- 串口驱动 (COM1, 115200 baud) - 调试输出
- ATA PIO 驱动 - 磁盘读写
- PS/2 键盘驱动 - 用户输入
- VGA 文本模式 - 显示输出

**未实现** (计划中):
- 网络驱动 (LWIP)
- USB 驱动
- 音频驱动

## 三、初始化顺序 (main.c)

```c
void kernel_main(void) {
    // 1. 基础硬件初始化
    gdt_init();
    idt_init();
    
    // 2. 内核子系统
    serial_init();
    klog_init();
    pmm_init();
    vmm_init();
    heap_init();
    
    // 3. 驱动和文件系统
    ata_init();           // ATA 磁盘检测
    hvfs_init();          // HvFS 初始化
    vfs_init();           // VFS 层初始化
    
    // 4. [v2.0] 智能挂载
    smart_mount_root();   // ← 根据模式选择 FS
    
    // 5. 用户进程
    start_user_processes(); // 启动 init/shell
}
```

## 四、代码量统计

| 模块 | 文件数 | 估计行数 | 状态 |
|------|--------|----------|------|
| 内核核心 | ~15 | ~8000 | ✅ 完成 |
| 文件系统 | ~10 | ~6000 | ✅ 完成 (含Smart Mount) |
| 驱动 | ~4 | ~2000 | 🔄 进行中 |
| Rust库 | ~20 | ~5000 | ✅ 完成 |
| **总计** | **~50** | **~21000** | **目标<50K** |

## 五、与其他OS对比

| 维度 | Linux | AntX |
|------|-------|------|
| 架构 | 宏内核 (30M行) | 宏内核 (<50K行) |
| 语言 | C + 汇编 | C + Rust |
| FS | VFS + ext4/btrfs... | VFS + HvFS/RamFS |
| 挂载 | mount(2) syscall | smart_mount (3模式) |
| 目标 | 通用服务器/桌面 | 个人学习探索 |

---

**文档维护者**: AI Assistant  
**创建日期**: 2026-05-06  
**基于规范**: ai-autonomous-development-spec.md v2.0
