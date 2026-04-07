# AntX 工作进度 - 001-项目初始化

## 概述

AntX 是一个 x86_64 架构的操作系统内核原型项目，采用 Multiboot2 启动协议，实现了从 32 位保护模式到 64 位长模式的切换。

## 已完成工作

### 1. 开发环境搭建

| 组件 | 版本/说明 |
|------|-----------|
| 操作系统 | Windows 11 + WSL2 |
| 编译器 | x86_64-linux-gnu-gcc |
| 汇编器 | NASM 2.x |
| 模拟器 | QEMU x86_64 |
| 引导工具 | GRUB (grub-pc-bin, xorriso) |

### 2. 项目结构

```
AntX/
├── docs/                    # 设计文档
├── logs/                    # 日志目录
├── scripts/                 # 构建脚本
├── src/
│   ├── include/             # 头文件
│   │   ├── gdt.h           # GDT 定义
│   │   ├── hvfs.h          # HvFS 文件系统定义
│   │   ├── idt.h           # IDT 定义
│   │   ├── io.h            # I/O 端口操作
│   │   ├── kernel.h        # 内核主头文件
│   │   ├── mm.h            # 内存管理定义
│   │   ├── printk.h        # 格式化输出
│   │   ├── proc.h          # 进程/会话定义
│   │   ├── pwid.h          # PWID权限模型定义
│   │   ├── serial.h        # 串口驱动接口
│   │   ├── stdarg.h        # 可变参数
│   │   ├── string.h        # 字符串库
│   │   ├── syscall.h       # 系统调用接口
│   │   └── types.h         # 基本类型定义
│   ├── kernel/              # 内核源码
│   │   ├── boot.asm        # 引导代码 (Multiboot2 + 长模式切换)
│   │   ├── gdt.asm         # GDT 汇编代码
│   │   ├── gdt.c           # GDT 实现
│   │   ├── idt.c           # IDT 实现
│   │   ├── isr.asm         # 中断服务程序
│   │   ├── main.c          # 内核入口
│   │   ├── serial.c        # 串口驱动实现
│   │   └── syscall.c       # 系统调用实现
│   ├── lib/                 # 库函数
│   │   ├── printk.c        # 格式化输出实现
│   │   └── string.c        # 字符串库实现
│   ├── mm/                  # 内存管理
│   │   ├── pmm.c           # 物理内存管理器
│   │   └── vmm.c           # 虚拟内存管理
│   ├── proc/                # 进程管理
│   │   ├── process.c       # 进程管理
│   │   ├── scheduler.c     # 调度器
│   │   ├── session.c       # 会话管理
│   │   └── switch.asm      # 进程切换汇编
│   ├── pwid/                # PWID权限模型
│   │   └── pwid.c          # PWID核心实现
│   ├── hvfs/                # HvFS文件系统
│   │   └── hvfs.c          # HvFS核心实现
│   └── link.ld             # 链接脚本
├── Makefile                 # 构建配置
└── .gitignore              # Git 忽略配置
```

### 3. 内核功能

| 功能 | 状态 | 说明 |
|------|------|------|
| Multiboot2 启动 | ✅ 完成 | GRUB 引导协议 |
| 32→64 位切换 | ✅ 完成 | 页表设置 + PAE + 长模式 |
| 串口输出 | ✅ 完成 | COM1 端口调试输出 |
| GDT | ✅ 完成 | 全局描述符表 |
| IDT | ✅ 完成 | 中断描述符表 |
| 内存管理 | ✅ 完成 | 物理内存分配器 + 虚拟内存 |
| 进程调度 | ✅ 完成 | 进程/会话管理 + 调度器 |
| 中断等待 | ✅ 完成 | 低功耗主循环 + 进程调度兼容 |
| PWID 权限 | ✅ 完成 | 独创权限模型 + SHA256哈希 |
| 文件系统 | ✅ 完成 | HvFS + PWID集成权限 |
| 系统调用 | ✅ 完成 | syscall接口 + 格式化输出 |

### 4. 构建系统

| 命令 | 功能 |
|------|------|
| `make all` | 构建内核 |
| `make iso` | 创建可启动 ISO |
| `make run-iso` | 通过 ISO 运行内核 |
| `make debug` | GDB 调试模式 |
| `make log` | 保存日志到 logs/serial.log |
| `make clean` | 清理构建目录 |

## 当前状态

内核成功启动并输出：

```
AntX OS v0.1.0
Copyright (c) 2024 AntX Project
========================================
[BOOT] Initializing kernel...
  [OK] GDT
  [OK] IDT
  [OK] PMM - 32496 pages free
  [OK] VMM
  [OK] Process Manager
  [OK] Session Manager
  [OK] Scheduler
  [OK] PWID Manager
  [OK] HvFS
  [OK] Syscall

[INIT] Creating original root...
  Default password: 'root'
[INIT] System initialized
AntX is ready.
Login with: pwid_login("note", "password")
File operations: hvfs_open/hvfs_close
Directory ops: hvfs_mkdir/hvfs_rmdir
Enabling interrupts...
[DONE] System running.
```

## 技术要点

### 启动流程

```
GRUB/Multiboot2 → 32位保护模式 → 页表初始化 → 启用PAE → 启用长模式 → 64位内核
```

### 关键文件

| 文件 | 行数 | 说明 |
|------|------|------|
| boot.asm | ~110 | Multiboot2 头 + 长模式切换代码 |
| gdt.asm | ~30 | GDT 加载汇编代码 |
| gdt.c | ~50 | GDT 初始化实现 |
| idt.c | ~120 | IDT 初始化和中断处理 |
| isr.asm | ~150 | 中断服务程序存根 |
| main.c | ~100 | 内核入口、中断等待、主循环 |
| serial.c | ~75 | 串口驱动实现 |
| pmm.c | ~60 | 物理内存管理器(位图分配器) |
| vmm.c | ~60 | 虚拟内存管理 |
| process.c | ~100 | 进程管理实现 |
| scheduler.c | ~110 | 调度器实现(时间片轮转) |
| session.c | ~70 | 会话管理实现 |
| switch.asm | ~80 | 进程上下文切换汇编 |
| pwid.c | ~550 | PWID权限模型核心实现 |
| hvfs.c | ~900 | HvFS文件系统实现 |
| syscall.c | ~100 | 系统调用实现 |
| string.c | ~130 | 字符串库函数 |
| printk.c | ~150 | 格式化输出实现 |
| link.ld | ~40 | ELF64 链接脚本 |

## 下一步计划

1. **系统调用接口** - 用户态与内核态交互
2. **设备驱动** - 键盘、定时器、磁盘
3. **Shell** - 命令行交互界面
4. **文件系统持久化** - HvFS数据持久化到磁盘
5. **网络协议栈** - 未来可扩展为网络协议栈
6. **多语言支持** - 国际化/多语言支持

## 备注

- 项目定位：原型探索项目，正在转向测试可用版本
- 开发环境：Windows 11 + WSL2
- 目标架构：x86_64 长模式
