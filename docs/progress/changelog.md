# AntX 变更日志

本文件记录 AntX 操作系统的重要变更历史。

---

## [Unreleased]

### Changed - 变更

#### 命名规范确立 (2026-04-19)

**内核命名**: QueenX (简称 QX)
- QueenX 是 AntX 操作系统的内核
- QueenX + 用户态组件 = AntX 完整操作系统
- 类似于 Linux 内核 + GNU 工具 = GNU/Linux

| 组件 | 名称 | 说明 |
|------|------|------|
| 内核 | **QueenX (QX)** | Ring 0 特权级运行的内核代码 |
| Shell | **antxsh** | AntX Shell |
| 完整系统 | **AntX** | QueenX + 用户态组件 |

### Added - 新增功能
- 测试框架初步实现
  - `tests/scripts/diagnose_user_process.py` - ELF一致性检查与自动修复工具
  - `tests/scripts/test_user_process.py` - QEMU自动化测试脚本
- **串口输入支持** (2026-04-19)
  - `sys_fs_read()` 现在同时支持键盘和串口输入
  - 优先使用键盘输入，串口作为备选
  - 用户态进程在 QEMU 串口终端中可正常交互
- **用户态安装向导** (2026-04-19)
  - `src/user/install/user_install.c` - 完整的用户态安装向导实现
  - 支持设置 root 密码、配置主机名、创建安装标记
  - 通过系统调用接口与内核交互

### Changed - 变更
- **内核启动架构重构** (2026-04-11)
  - 采用 Linux/Windows/BSD 标准的双映射启动方案
  - 实现恒等映射 + 高地址映射的双页表结构
  - 使用 2MB 大页映射 1TB 物理内存
  - 添加内核代码从 LMA 到 VMA 的复制机制
  - 添加 TLB 刷新确保映射正确性
  - 更新链接脚本支持 VMA/LMA 分离
  - 更新文档：
    - `docs/development/memory-management.md` - 新增双映射机制章节
    - `docs/development/kernel-architecture.md` - 更新启动流程说明
- 重写 `process_start_user_asm` (switch.asm)
  - 修复 iretq 栈帧构建顺序
  - 使用 rbx/r12 保存关键寄存器

### Fixed - 修复

#### 键盘驱动修复 (2026-04-19)
| 日期 | 修复项 | 文件 | 说明 |
|------|--------|------|------|
| 2026-04-19 | Backspace 扫描码映射 | src/kernel/keyboard.c | `0x0E` → `'\b'` |
| 2026-04-19 | Tab 扫描码映射 | src/kernel/keyboard.c | `0x0F` → `'\t'` |

**影响**: 用户现在可以在安装向导和 Shell 中正常使用退格键和 Tab 键

#### 用户态进程输入修复 (2026-04-19)
| 日期 | 修复项 | 文件 | 说明 |
|------|--------|------|------|
| 2026-04-19 | sys_fs_read 双源输入 | src/kernel/syscall.c | 支持键盘+串口输入 |
| 2026-04-19 | serial_has_data 函数 | src/kernel/serial.c | 串口数据检测 |
| 2026-04-19 | serial_getc 函数 | src/kernel/serial.c | 串口字符读取 |

**影响**: 用户态程序（安装向导、Shell）可在串口终端环境中接收输入

#### 用户态进程启动问题 (2026-04-13)

| 日期 | 修复项 | 文件 |
|------|--------|------|
| 2026-04-13 | USER_STACK_TOP 规范地址修复 | src/include/user_proc.h |
| 2026-04-13 | TSS 描述符 64 位地址设置 | src/kernel/gdt.c |
| 2026-04-13 | iretq 前设置 DS/ES | src/proc/scheduler.c |
| 2026-04-13 | 禁用内核 stack canary | Makefile |
| 2026-04-13 | boot.asm kernel_main 地址修复 | src/kernel/boot.asm |
| 2026-04-13 | boot.asm 栈地址高地址修复 | src/kernel/boot.asm |
| 2026-04-13 | boot.asm invlpg 语法修复 | src/kernel/boot.asm |
| 2026-04-13 | gdt.asm retfq 语法修复 | src/kernel/gdt.asm |

**状态**: ✅ 已解决 - 用户态进程可正常运行

### Verified - 验证确认 (2026-04-19)

#### 安装向导与 Shell 用户态运行验证
- ✅ 安装向导 (`install_guide_run`) 在 Ring 3 (CPL=3) 运行
- ✅ Shell (`antxsh`) 在 Ring 3 (CPL=3) 运行
- ✅ 用户态上下文正确设置：CS=`GDT_USER_CODE|0x03`, SS=`GDT_USER_DATA|0x03`
- ✅ 通过 `iretq` 指令完成特权级切换

#### 系统安装与持久化能力验证
- ✅ ATA 磁盘读写完整实现 (`src/disk/ata.c`)
- ✅ HvFS 磁盘同步功能 (`hvfs_sync()`)
- ✅ 安装向导三步流程：root 账户 → 系统配置 → 完成安装
- ✅ 安装标记检测 (`/.antx_installed`)
- ✅ 启动时自动挂载 DiskFS 或回退 RamFS

#### 内核架构分层设计验证
- ✅ 内核态/用户态边界清晰 (int $0x80 系统调用门)
- ✅ VFS 抽象层完整 (vfs_file_operations / vfs_inode_operations)
- ✅ 多文件系统注册 (RamFS, DiskFS, DevFS, ProcFS)
- ✅ PWID 权限模型集成到系统调用
- ✅ C/Rust 双语言架构分工明确

---

## [0.1.0] - 2026-04-06

### Added - 新增功能
- 基础内核架构
  - GDT/IDT 初始化
  - 物理内存管理 (PMM)
  - 虚拟内存管理 (VMM)
  - 进程管理与调度器
- PWID 权限模型
  - 三级权限体系 (ROOT/TRUSTWORTHY/UNTRUSTWORTHY)
  - SHA256 密码哈希
- 文件系统
  - VFS 虚拟文件系统层
  - RamFS 内存文件系统
  - DiskFS 磁盘文件系统
- 用户进程支持
  - ELF 加载器
  - 用户模式切换

### Known Issues - 已知问题
- 用户进程启动后无输出 (Issue #9)
- 开机调试信息缺乏真正的错误检测 (Issue #12)

---

## 版本说明

遵循 [语义化版本](https://semver.org/lang/zh-CN/) 规范：

- **主版本号**: 不兼容的 API 修改
- **次版本号**: 向下兼容的功能性新增
- **修订号**: 向下兼容的问题修正

---

*最后更新: 2026-04-07*
