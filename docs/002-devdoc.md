# AntX 工作进度 - 002-Shell与键盘驱动

## 文档状态

| 项目 | 状态 |
|------|------|
| 键盘驱动 | ✅ 已完成 |
| antxsh Shell | ✅ 已完成 |
| 系统调用重构 | ✅ 已完成 |
| 安全机制 | ✅ 已完成 |
| HvFS 磁盘化 | ✅ 基础实现完成 |
| 系统安装向导 | ✅ 已完成 |
| 文件系统分层架构 | ✅ 已完成 |
| 用户态进程 | ⏳ 待实现 |

## 概述

本阶段在 AntX 内核基础上实现了以下关键功能：
1. **键盘驱动** - PS/2 键盘中断驱动，支持扫描码转换和环形缓冲区
2. **antxsh Shell** - 交互式命令行界面，内置命令系统
3. **系统调用重构** - 焕新版命名风格，完善的错误码体系
4. **安全机制** - 首次登录强制设置密码，非硬编码初始密码

## 已完成功能

### 1. 键盘驱动

#### 1.1 实现内容

| 功能 | 文件 | 描述 |
|------|------|------|
| 中断处理 | `keyboard.c` | IRQ1 中断处理程序 |
| 扫描码转换 | `keyboard.c` | Set 1 扫描码转 ASCII |
| 修饰键支持 | `keyboard.c` | Shift、Caps Lock、Ctrl |
| 环形缓冲区 | `keyboard.c` | 256 字节输入缓冲 |
| 行编辑 | `keyboard.c` | 退格删除、回车确认 |

#### 1.2 键盘端口

| 端口 | 用途 |
|------|------|
| 0x60 | 数据端口（读取扫描码） |
| 0x64 | 状态/命令端口 |

#### 1.3 数据结构

```c
struct keyboard_buffer {
    char buffer[KEYBOARD_BUFFER_SIZE];  // 256 bytes
    int head;
    int tail;
    int count;
};
```

### 2. antxsh Shell

#### 2.1 内置命令

| 命令 | 描述 | 权限要求 |
|------|------|----------|
| `help` | 显示帮助信息 | 无 |
| `clear` | 清屏 | 无 |
| `echo [text]` | 输出文本 | 无 |
| `exit` | 退出 Shell | 无 |
| `auth_login "note" "password"` | 登录 | 无 |
| `auth_logout` | 注销 | 无 |
| `auth_whoami` | 显示当前 PWID | 无 |
| `auth_passwd` | 修改密码 | 已登录 |
| `hostname [name]` | 显示/设置主机名 | Root 设置 |

#### 2.2 提示符设计

```
[>]                        # 未登录
[root@localhost /] #       # Root 权限
[user@localhost ~] $       # Trustworthy 权限
[guest@localhost ~] %      # Untrustworthy 权限
```

#### 2.3 命令解析

- 支持引号包裹参数
- 支持空格分隔
- 最大 16 个参数

### 3. 系统调用重构

#### 3.1 调用号分配

| 类别 | 调用号范围 | 已实现 |
|------|-----------|--------|
| 进程管理 | 0-10 | `proc_exit`, `proc_getid`, `proc_getppid`, `proc_getpwid`, `proc_yield` |
| 文件系统 | 20-33 | `fs_open`, `fs_close`, `fs_read`, `fs_write`, `fs_mkdir`, `fs_rmdir`, `fs_unlink`, `fs_stat`, `fs_chmod`, `fs_chown`, `fs_rename`, `fs_seek`, `fs_readdir` |
| 权限认证 | 40-49 | `auth_login`, `auth_logout`, `auth_create`, `auth_delete`, `auth_list`, `auth_info`, `auth_setnote`, `auth_changepw`, `auth_verify` |
| 内存管理 | 60-63 | 框架已定义 |
| IPC/网络 | 80-88 | 框架已定义 |
| 环境/系统 | 100-109 | `env_getcwd`, `env_chdir`, `gethostname`, `sethostname` |
| 设备操作 | 120-122 | 框架已定义 |

#### 3.2 错误码体系

```c
// 通用错误码
#define E_PERM             (-1)   // 权限不足
#define E_NOTFOUND         (-2)   // 未找到
#define E_INTR             (-4)   // 中断
#define E_IO               (-5)   // I/O 错误
#define E_INVAL            (-22)  // 无效参数

// 权限相关错误码
#define E_AUTH_INVALID     (-100) // 无效认证
#define E_AUTH_NOTFOUND    (-101) // 用户不存在
#define E_AUTH_DISABLED    (-102) // 账户禁用
#define E_AUTH_PWERR       (-104) // 密码错误
#define E_AUTH_NOROOT      (-105) // 需要 Root 权限
```

### 4. 安全机制

#### 4.1 初始 Root 创建

- 空密码创建初始 root 账户
- 设置 `PWID_FLAG_DEFAULT_PW` 标志
- 首次登录强制设置密码

#### 4.2 密码设置流程

```
首次登录检测
    │
    ├── 检测到 DEFAULT_PW 标志
    │       │
    │       ▼
    │   显示安全提示
    │       │
    │       ▼
    │   要求设置新密码
    │       │
    │       ├── 密码长度 >= 4
    │       ├── 确认密码匹配
    │       └── 清除 DEFAULT_PW 标志
    │
    └── 正常进入 Shell
```

#### 4.3 PWID 标志位

| 标志 | 值 | 描述 |
|------|-----|------|
| `PWID_FLAG_ORIGINAL_ROOT` | 0x01 | 初始 root，不可删除 |
| `PWID_FLAG_TEMPORARY` | 0x02 | 临时账户 |
| `PWID_FLAG_DISABLED` | 0x04 | 禁用账户 |
| `PWID_FLAG_MODIFIED` | 0x08 | 已修改 |
| `PWID_FLAG_DEFAULT_PW` | 0x10 | 使用默认密码 |

## 项目结构

```
src/
├── include/
│   ├── keyboard.h          # 键盘驱动接口
│   ├── syscall.h           # 系统调用接口
│   ├── shell.h             # Shell 接口
│   ├── pwid.h              # PWID 权限系统
│   ├── vfs.h               # VFS 统一接口层 ✅
│   ├── hvfs.h              # HvFS 磁盘文件系统接口
│   ├── user_proc.h         # 用户态进程框架
│   ├── install_guide.h     # 安装向导接口 ✅
│   └── user/
│       └── user.h          # 用户态库接口
├── kernel/
│   ├── keyboard.c          # 键盘驱动实现 ✅
│   ├── shell.c             # Shell 实现 ✅
│   ├── syscall.c           # 系统调用实现 ✅
│   ├── main.c              # 内核入口
│   └── install_guide.c     # 安装向导实现 ✅
├── fs/
│   ├── vfs.c               # VFS 核心实现 ✅
│   ├── ramfs.c             # 内存文件系统 ✅
│   ├── diskfs.c            # 磁盘文件系统 ✅
│   ├── devfs.c             # 设备文件系统 ✅
│   └── procfs.c            # 进程文件系统 ✅
├── proc/
│   ├── process.c           # 进程管理
│   ├── scheduler.c         # 调度器
│   └── user_proc.c         # 用户态进程框架
├── pwid/
│   └── pwid.c              # PWID 实现 ✅
├── user/
│   ├── antxsh/             # 用户态 Shell（预留）
│   │   ├── main.c
│   │   ├── builtins.h
│   │   └── builtins.c
│   └── lib/
│       └── user.c          # 用户态库
├── hvfs/
│   └── hvfs.c              # HvFS 底层实现
└── disk/
    └── ata.c               # ATA 驱动
```

## 运行结果

### 首次启动（安装向导）

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
  [OK] ATA Driver
  [OK] VFS Layer
  [OK] Filesystem mounts
  [OK] Keyboard

[INIT] System initialized
AntX is ready.

Enabling interrupts...
[DONE] System running.

[INSTALL] First boot detected. Running installation wizard...

========================================
        AntX Installation Wizard
========================================

Welcome to AntX Operating System!

This wizard will guide you through the
initial system setup. This process will
only run once.

Press ENTER to continue...


--- Step 1: Root Account Setup ---

Creating the root (administrator) account.
This account has full system access.

Enter root password (min 4 chars): ****
Confirm root password: ****
Enter root account note (default: root): admin

Creating root account...
Root account created successfully!

--- Step 2: System Configuration ---

Enter hostname (default: localhost): myserver
Hostname set to: myserver

--- Step 3: Finalizing Installation ---

Syncing filesystem to disk...
Creating installation marker...

========================================
     Installation Complete!
========================================

AntX is now ready for use.

Starting shell in 3 seconds...

[INSTALL] Installation complete. Starting shell...

========================================
antxsh v0.1.0 - AntX Shell
Type 'help' for available commands.
========================================

[>] auth_login "admin" "****"
Login successful! PWID: 0x...
[admin@myserver /] # 
```

### 后续启动（跳过安装向导）

```
AntX OS v0.1.0
Copyright (c) 2024 AntX Project
========================================
[BOOT] Initializing kernel...
  [OK] GDT
  [OK] IDT
  ...
  
[INIT] System initialized
AntX is ready.

Enabling interrupts...
[DONE] System running.

[BOOT] System already installed. Starting shell...

========================================
antxsh v0.1.0 - AntX Shell
Type 'help' for available commands.
========================================

[>] 
```

## 下一步计划

### 阶段一：HvFS 磁盘化 ✅ 已完成

将内存文件系统改为磁盘持久化存储。

#### 已完成工作

| 任务 | 状态 | 描述 |
|------|------|------|
| 磁盘布局常量定义 | ✅ | `hvfs.h` 中定义扇区布局 |
| ATA/IDE 驱动 | ✅ | `ata.c` 实现基本读写操作 |
| 磁盘检测与格式化 | ✅ | `hvfs_check_disk()`, `hvfs_format_disk()` |
| 数据持久化 | ✅ | `hvfs_sync()` 同步到磁盘 |
| 挂载与卸载 | ✅ | `hvfs_mount()`, `hvfs_unmount()` |
| Shell 文件命令 | ✅ | `ls`, `cd`, `cat`, `touch`, `mkdir`, `rm`, `sync`, `pwd`, `write` |
| 系统调用完善 | ✅ | `sys_env_getcwd`, `sys_env_chdir`, `sys_fs_sync` |
| 默认目录创建 | ✅ | `/bin`, `/sbin`, `/etc`, `/home`, `/tmp`, `/dev`, `/proc`, `/sys` |

#### 磁盘镜像布局

```
┌─────────────────────────────────────────────────────────────┐
│                    AntX 磁盘镜像布局                          │
├─────────────────────────────────────────────────────────────┤
│ 扇区 0-1     │ MBR + 引导代码                                │
│ 扇区 2-9     │ HvFS 超级块 (4KB)                             │
│ 扇区 10-137  │ Inode 表 (64KB, 1024个inode)                  │
│ 扇区 138-165 │ 块位图 (14KB, 管理1024个块)                    │
│ 扇区 166+    │ 数据区 (文件内容)                              │
└─────────────────────────────────────────────────────────────┘
```

#### 已修复的关键问题

| 问题 | 描述 | 修复方案 |
|------|------|----------|
| 栈溢出崩溃 | 多个函数中 ATA 读取数据量超过栈上结构体大小 | 使用静态缓冲区替代栈变量 |
| 缓冲区溢出 | 位图数组大小与读取扇区数不匹配 | 使用中间缓冲区，只拷贝需要的大小 |

### 阶段二：文件系统分层架构重构 ✅ 已完成

当前问题：内存文件系统与磁盘文件系统是**互斥模式**，无法协作。

#### 已实现的架构

```
┌─────────────────────────────────────────────────────────────┐
│                    VFS 统一接口层                             │
│  open() │ close() │ read() │ write() │ mkdir() │ unlink()   │
├─────────────────────────────────────────────────────────────┤
│                      挂载点管理器                             │
│              根据路径前缀路由到对应文件系统                     │
├───────────────────┬─────────────────────────────────────────┤
│   内存文件系统     │           磁盘文件系统                    │
│   (RamFS)         │           (DiskFS)                       │
├───────────────────┼─────────────────────────────────────────┤
│ /dev  /proc       │  /bin  /sbin  /etc  /home               │
│ /sys  /tmp        │                                         │
└───────────────────┴─────────────────────────────────────────┘
```

#### 已完成的工作

| 步骤 | 任务 | 文件 | 状态 |
|------|------|------|------|
| 1.1 | 定义 VFS 抽象接口 | `vfs.h` | ✅ 已完成 |
| 1.2 | 实现挂载点管理 | `vfs.c` | ✅ 已完成 |
| 1.3 | 重构 RamFS | `ramfs.c` | ✅ 已完成 |
| 1.4 | 重构 DiskFS | `diskfs.c` | ✅ 已完成 |
| 1.5 | 实现设备文件支持 | `devfs.c` | ✅ 已完成 |
| 1.6 | 实现 procfs | `procfs.c` | ✅ 已完成 |
| 1.7 | 更新系统调用 | `syscall.c` | ✅ 已完成 |
| 1.8 | 更新内核初始化 | `main.c` | ✅ 已完成 |

#### 实现的收益

| 收益 | 描述 |
|------|------|
| 内存优化 | 磁盘文件按需加载，不再全量缓存 |
| 职责清晰 | 动态数据与持久化数据分离 |
| 可扩展性 | 支持未来添加网络文件系统等 |
| 设备抽象 | `/dev` 支持设备节点 |

#### 挂载点配置

| 路径 | 文件系统 | 用途 |
|------|----------|------|
| `/` | DiskFS | 根文件系统，持久化存储 |
| `/dev` | DevFS | 设备文件系统，动态设备节点 |
| `/proc` | ProcFS | 进程信息文件系统 |
| `/tmp` | RamFS | 临时文件，内存存储 |

### 阶段三：系统安装向导 ✅ 已完成

> 参考 **devdoc.md**《AntX 安装向导设计指南》实现

安装向导在内核初始化后、Shell 启动前运行，完成首次配置。

#### 设计原则

| 原则 | 描述 |
|------|------|
| 极简流程 | 固定 3 步：欢迎页 → Root PWID → 系统配置 → 完成 |
| 一次性运行 | 通过标记文件 `/.antx_installed` 防止重复执行 |
| 容错兜底 | 密码校验、默认值兜底、避免配置失败 |
| 内核态实现 | 基于 `keyboard_read_line`/`sys_auth_create`/`sys_fs_open` 接口 |

#### 已完成工作

| 任务 | 状态 | 文件 |
|------|------|------|
| 安装向导头文件 | ✅ | `install_guide.h` |
| 安装向导实现 | ✅ | `install_guide.c` |
| Root PWID 创建 | ✅ | 使用 `sys_auth_create` |
| 系统配置（主机名） | ✅ | 写入 `/etc/hostname`，默认 `localhost` |
| 标记文件检测 | ✅ | 检测 `/.antx_installed` |
| 集成到启动流程 | ✅ | `main.c` |

#### 安装流程

```
内核启动完成
      │
      ▼
检测 /.antx_installed 标记
      │
      ├── 存在 → 启动 antxsh
      │
      └── 不存在 → 运行安装向导
                    │
                    ▼
              欢迎页 → Root PWID 配置 → 基础配置 → 完成页
                                                      │
                                                      ▼
                                            创建标记文件
                                                      │
                                                      ▼
                                            启动 antxsh
```

### 阶段四：用户态进程 ⏳ 待实现

将 antxsh 移至用户态运行。

#### 技术架构

```
用户态 (Ring 3)
┌─────────────────────────────────────────────────────┐
│  antxsh  │  init  │  其他用户程序                      │
└─────────────────────────────────────────────────────┘
            │ int 0x80
            ▼
───────────────────────────────────────────────────────
内核态 (Ring 0)
┌─────────────────────────────────────────────────────┐
│  系统调用处理 │ 进程管理 │ 内存管理 │ 文件系统        │
└─────────────────────────────────────────────────────┘
```

#### 实现步骤

| 步骤 | 任务 | 文件 | 依赖 |
|------|------|------|------|
| 4.1 | 添加 TSS 段到 GDT | `gdt.c` | 无 |
| 4.2 | 实现用户态栈切换 | `gdt.asm` | 4.1 |
| 4.3 | 实现进程独立地址空间 | `vmm.c` | 无 |
| 4.4 | 实现 `fork` 系统调用 | `syscall.c` | 4.3 |
| 4.5 | 实现 `exec` 系统调用 | `syscall.c` | ELF加载器 |
| 4.6 | 实现简单 ELF 加载器 | `elf.c` | 无 |
| 4.7 | 创建 init 进程 | `init.c` | 4.4-4.6 |
| 4.8 | 将 antxsh 移至用户态 | `user/antxsh/` | 4.4-4.6 |

## 内核设计原则

### 裁判/运动员分离

| 角色 | 职责 |
|------|------|
| 内核（裁判）| 硬件抽象、资源分配、权限校验 |
| 用户态（运动员）| 业务逻辑、命令执行、用户体验 |

### 三件事原则

1. **只管「硬件抽象」，不管「硬件使用」**
2. **只管「资源分配」，不管「资源使用」**
3. **只管「权限校验」，不管「权限使用」**

## 备注

- AntX Shell 简称 **antxsh**
- 键盘驱动基于标准 PS/2 键盘
- 当前 Shell 运行在内核态，待用户态进程实现后迁移
- 优先实现核心功能，再逐步完善

## 参考文档

> 本文档涉及的部分设计理念参考 **devdoc.md**：

| devdoc 文档 | 参考内容 |
|-------------|----------|
| 《Linux 内核管辖范围与 AntX 极简设计》 | 内核只做3件事原则、模块极简设计 |
| 《AntX 安装向导设计指南》 | 安装向导的用户态实现、流程设计 |

详细设计请参阅 [devdoc.md](devdoc.md)。
