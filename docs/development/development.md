# AntX 开发指导

## 一、开发环境搭建

### 1.1 推荐环境

**Fedora Linux (推荐)**

本项目推荐在 Fedora Linux 上进行开发，这是经过验证的稳定环境。其他基于 RPM 的发行版（如 RHEL、CentOS Stream）也可使用。

### 1.2 所需工具

| 工具 | 用途 | 安装方式 |
|------|------|----------|
| GCC 交叉编译器 | C 编译器 | `sudo dnf install gcc-x86_64-linux-gnu` |
| NASM | 汇编器 | `sudo dnf install nasm` |
| QEMU | x86_64 模拟器 | `sudo dnf install qemu-system-x86` |
| GRUB | 引导加载程序 | `sudo dnf install grub2-tools xorriso` |
| GDB | 调试器 | `sudo dnf install gdb` |
| make | 构建工具 | `sudo dnf install make` |

### 1.3 环境安装

#### 方式一：使用依赖检查脚本（推荐）⭐ v2.0 新功能

```bash
# 运行交互式依赖检查与安装脚本
bash scripts/requirements.sh

# 可选参数:
#   --auto           自动安装所有缺失依赖（无需确认）
#   --verbose        显示详细的检测和安装过程
#   --skip-optional  跳过可选依赖（如 Rust 工具链）
#   --force          强制重新检测（忽略缓存）

# 示例：全自动安装
bash scripts/requirements.sh --auto --verbose

# 示例：仅检测，不安装（查看缺失项）
bash scripts/requirements.sh --skip-optional
```

**脚本特性**：
- ✅ **智能检测**: 自动识别 15+ 个必需和可选工具
- ✅ **交互式询问**: 对每个缺失依赖提示是否安装
- ✅ **彩色输出**: 清晰显示 ✓ 已安装 / ✗ 未找到 / ○ 可选
- ✅ **分类显示**: 区分必需依赖（编译必须）和可选依赖（增强功能）
- ✅ **支持发行版**: Fedora / RHEL / CentOS Stream 等 RPM 系统
- ✅ **可选依赖**: Rust 工具链、GDB 调试器等标记为可选

**输出示例**：
```
╔══════════════════════════════════════════════╗
║     AntX 内核构建环境依赖检查工具 v2.0       ║
╚══════════════════════════════════════════════╝

━━━ 必需依赖 ━━━

  gcc-x86_64-linux-gnu              ✓ 已安装
  binutils-x86_64-linux-gnu         ✓ 已安装
  nasm                              ✓ 已安装
  make                              ✓ 已安装
  qemu-system-x86                   ✓ 已安装
  xorriso                           ✓ 已安装
  grub2-tools                       ✓ 已安装

━━━ 可选依赖 ━━━

  rustc                             ○ 未安装 (可选) - Rust 编译器
  gdb                               ✓ 已安装 - 调试器

✓ 所有必需依赖已满足！可以开始构建。
```

#### 方式二：手动安装

```bash
sudo dnf install -y make nasm qemu-system-x86 gdb xorriso grub2-tools \
    gcc-x86_64-linux-gnu binutils-x86_64-linux-gnu
```

### 1.4 环境验证

```bash
# 验证编译器
x86_64-linux-gnu-gcc --version

# 验证汇编器
nasm --version

# 验证 QEMU
qemu-system-x86_64 --version

# 验证 GRUB (Fedora 使用 grub2-mkrescue)
grub2-mkrescue --version

# 验证 GDB
gdb --version
```

## 二、项目结构

```
AntX/
├── README.md                     # 项目说明
├── Makefile                      # 构建配置
├── LICENSE                       # 许可证
├── .gitignore                    # Git 忽略配置
│
├── docs/                         # 文档目录
│   ├── README.md                 # 文档导航
│   ├── development/              # 开发文档
│   │   ├── README.md             # 开发文档索引
│   │   ├── development.md        # 开发指导
│   │   ├── devdoc.md             # 开发文档
│   │   ├── kernel-architecture.md # 内核架构设计
│   │   ├── memory-management.md  # 内存管理
│   │   ├── process-session.md    # 进程与会话管理
│   │   ├── thread-scheduler.md   # 线程与调度器
│   │   ├── pwid-model.md         # PWID 权限模型
│   │   ├── pwid-enhanced-v2.md   # PWID 增强版
│   │   ├── syscall.md            # 系统调用接口
│   │   ├── hivefs.md             # HiveFS 文件系统
│   │   ├── hvfs-disk.md          # HVFS 磁盘格式
│   │   ├── ipc.md                # 进程间通信
│   │   ├── keyboard.md           # 键盘驱动
│   │   ├── security-mechanisms.md # 安全机制
│   │   ├── pic-implementation.md # PIC 位置无关代码
│   │   ├── pic-quick-start.md    # PIC 快速开始
│   │   ├── rust-filesystem.md    # Rust 文件系统重写
│   │   └── rust-process.md       # Rust 进程管理重写
│   ├── issues/                   # 问题文档
│   │   ├── README.md
│   │   ├── issue-recommend.md    # 问题追踪与建议
│   │   └── user-mode-gpf.md      # 用户态 GPF 问题
│   └── progress/                 # 进度文档
│       ├── README.md
│       ├── changelog.md          # 变更日志
│       ├── current-tasks.md      # 当前任务
│       ├── milestones.md         # 里程碑记录
│       └── antx-focused-priority.md # 优先级规划
│
├── scripts/                      # 构建与分析脚本
│   ├── requirements.sh           # 🔧 依赖检查与自动安装 v2.0 ⭐ 新增
│   ├── generate_version.sh       # 🔄 动态版本生成脚本 ⭐ 新增
│   ├── build.sh                  # 构建脚本
│   ├── gen_embed.py              # 嵌入二进制生成
│   ├── analyze_*.py              # 各类分析脚本
│   ├── diagnose_*.py             # 诊断脚本
│   └── verify_*.py               # 验证脚本
│
├── src/                          # 源代码
│   ├── include/                  # 头文件
│   │   ├── kernel.h              # 内核主头文件
│   │   ├── types.h               # 基本类型定义
│   │   ├── config.h              # 配置选项
│   │   ├── io.h                  # 端口 I/O
│   │   ├── serial.h              # 串口驱动
│   │   ├── gdt.h                 # 全局描述符表
│   │   ├── idt.h                 # 中断描述符表
│   │   ├── mm.h                  # 内存管理
│   │   ├── proc.h                # 进程管理
│   │   ├── thread.h              # 线程管理
│   │   ├── scheduler_ex.h        # 扩展调度器
│   │   ├── pwid.h                # PWID 权限
│   │   ├── vfs.h                 # 虚拟文件系统
│   │   ├── hvfs.h                # HvFS 文件系统
│   │   ├── syscall.h             # 系统调用
│   │   ├── ipc.h                 # 进程间通信
│   │   ├── keyboard.h            # 键盘驱动
│   │   ├── timer.h               # 定时器
│   │   ├── ata.h                 # ATA 磁盘驱动
│   │   ├── kmalloc.h             # 内核内存分配
│   │   ├── log_buffer.h          # 日志缓冲区
│   │   ├── user_proc.h           # 用户进程
│   │   ├── assert.h              # 断言宏
│   │   ├── errno.h               # 错误码
│   │   ├── string.h              # 字符串操作
│   │   ├── printk.h              # 内核打印
│   │   ├── user/                 # 用户态头文件
│   │   │   ├── syscall.h         # 用户态系统调用
│   │   │   └── user.h            # 用户态辅助
│   │   └── tests/                # 测试头文件
│   │       └── kernel_test.h
│   │
│   ├── kernel/                   # 内核核心
│   │   ├── boot.asm              # 启动代码 (Multiboot2)
│   │   ├── entry.asm             # 入口汇编
│   │   ├── gdt.asm               # GDT 汇编部分
│   │   ├── isr.asm               # 中断服务例程
│   │   ├── main.c                # 内核入口
│   │   ├── gdt.c                 # GDT 初始化
│   │   ├── idt.c                 # IDT 初始化
│   │   ├── serial.c              # 串口驱动
│   │   ├── keyboard.c            # 键盘驱动
│   │   ├── timer.c               # 定时器驱动
│   │   ├── syscall.c             # 系统调用处理
│   │   ├── stack_canary.c        # 栈保护
│   │   ├── log_buffer.c          # 日志缓冲区
│   │   └── tests/                # 内核测试
│   │       ├── kernel_test.c
│   │       ├── test_main.c
│   │       ├── test_pmm.c
│   │       ├── test_vmm.c
│   │       ├── test_kmalloc.c
│   │       ├── test_process.c
│   │       ├── test_scheduler.c
│   │       ├── test_vfs.c
│   │       ├── test_syscall.c
│   │       ├── test_ipc.c
│   │       ├── test_hvfs.c
│   │       ├── test_pwid_enhanced.c
│   │       └── test_persistence.c
│   │
│   ├── mm/                       # 内存管理
│   │   ├── pmm.c                 # 物理内存管理
│   │   ├── vmm.c                 # 虚拟内存管理
│   │   └── kmalloc.c             # 内核堆分配
│   │
│   ├── proc/                     # 进程管理
│   │   ├── process.c             # 进程管理 (C)
│   │   ├── process.rs            # 进程管理 (Rust)
│   │   ├── scheduler.c           # 调度器 (C)
│   │   ├── scheduler.rs          # 调度器 (Rust)
│   │   ├── scheduler_ex.c        # MLFQ 调度器
│   │   ├── thread.c              # 线程管理
│   │   ├── session.c             # 会话管理
│   │   ├── user_proc.c           # 用户进程
│   │   ├── init.c                # 进程初始化
│   │   ├── switch.asm            # 上下文切换
│   │   ├── ffi.rs                # FFI 绑定
│   │   ├── mod.rs                # Rust 模块
│   │   └── types.rs              # Rust 类型定义
│   │
│   ├── pwid/                     # PWID 权限模块
│   │   ├── pwid.c                # PWID 核心 (C)
│   │   ├── mod.rs                # Rust 模块
│   │   ├── types.rs              # 类型定义
│   │   ├── capability.rs         # 能力系统
│   │   ├── permission.rs         # 权限管理
│   │   ├── token.rs              # 令牌管理
│   │   ├── context.rs            # 上下文管理
│   │   ├── trust_chain.rs        # 信任链
│   │   └── ffi.rs                # FFI 绑定
│   │
│   ├── fs/                       # 文件系统 (模块内聚结构)
│   │   ├── mod.rs                # Rust 模块入口
│   │   ├── vfs/                  # VFS 核心层
│   │   │   ├── vfs.c             # C 实现
│   │   │   ├── vfs.rs            # Rust 实现
│   │   │   ├── types.rs          # 类型定义
│   │   │   ├── ffi.rs            # FFI 绑定
│   │   │   └── mod.rs
│   │   ├── ramfs/                # 内存文件系统
│   │   │   ├── ramfs.c
│   │   │   ├── ramfs.rs
│   │   │   └── mod.rs
│   │   ├── diskfs/               # 磁盘文件系统
│   │   │   ├── diskfs.c
│   │   │   ├── diskfs.rs
│   │   │   └── mod.rs
│   │   ├── hvfs/                 # HvFS 文件系统
│   │   │   ├── hvfs.c
│   │   │   ├── hvfs.rs
│   │   │   └── mod.rs
│   │   ├── devfs/                # 设备文件系统
│   │   │   ├── devfs.c
│   │   │   └── mod.rs
│   │   └── procfs/               # 进程文件系统
│   │       ├── procfs.c
│   │       └── mod.rs
│   │
│   ├── ipc/                      # 进程间通信
│   │   └── ipc.c                 # IPC 实现
│   │
│   ├── disk/                     # 磁盘驱动
│   │   └── ata.c                 # ATA/IDE 驱动
│   │
│   ├── lib/                      # 内核库
│   │   ├── string.c              # 字符串操作
│   │   └── printk.c              # 格式化打印
│   │
│   ├── rust/                     # Rust 运行时
│   │   ├── Cargo.toml            # Rust 配置
│   │   ├── Cargo.lock            # 依赖锁定
│   │   ├── .cargo/config.toml    # Cargo 配置
│   │   └── src/
│   │       ├── lib.rs            # Rust 库入口
│   │       └── memory_allocator.rs # 内存分配器
│   │
│   ├── user/                     # 用户态程序
│   │   ├── link.ld               # 用户程序链接脚本
│   │   ├── init/                 # init 进程
│   │   │   └── main.c
│   │   ├── antxsh/               # Shell
│   │   │   ├── main.c
│   │   │   ├── builtins.c
│   │   │   └── builtins.h
│   │   ├── install/              # 安装向导
│   │   │   ├── user_install.c
│   │   │   └── user_install.h
│   │   ├── lib/                  # 用户态库
│   │   │   ├── user.c
│   │   │   └── stack_canary.c
│   │   └── embedded/             # 嵌入的二进制
│   │       ├── user_init_bin.c
│   │       ├── user_antxsh_bin.c
│   │       └── user_install_bin.c
│   │
│   └── link.ld                   # 内核链接脚本
│
├── tests/                        # 测试框架
│   ├── run_tests.py              # 测试运行器
│   ├── integration/              # 集成测试
│   │   └── run_integration_tests.py
│   ├── stress/                   # 压力测试
│   │   └── run_stress_tests.py
│   ├── chaos/                    # 混沌测试
│   │   └── run_chaos_tests.py
│   ├── scripts/                  # 测试脚本
│   │   ├── diagnose_user_mode.py
│   │   ├── diagnose_user_process.py
│   │   └── test_user_process.py
│   └── reports/                  # 测试报告
│       └── unit_test_*.json
│
├── build/                        # 构建输出 (gitignore)
│   ├── kernel.bin                # 内核二进制
│   ├── kernel.map                # 符号映射
│   ├── user.map                  # 用户程序映射
│   ├── antx.iso                  # 可启动 ISO
│   └── antx.img                  # 磁盘镜像
│
├── logs/                         # 日志目录 (gitignore)
│   └── serial.log                # 串口日志
│
└── isodir/                       # ISO 临时目录 (gitignore)
    └── boot/grub/
        └── grub.cfg
```

## 三、内核启动流程

### 3.1 启动架构

AntX 是 **64 位内核**，采用 Multiboot2 协议启动：

```
┌─────────────────────────────────────────────────────────────┐
│                      启动流程                                 │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  GRUB/Multiboot2 ──▶ 32位保护模式 ──▶ 长模式切换 ──▶ 64位内核 │
│                                                               │
│  1. GRUB 加载内核 (Multiboot2 协议)                           │
│  2. boot.asm: 32位代码初始化页表                              │
│  3. 启用 PAE 和长模式                                         │
│  4. 跳转到 64 位代码                                          │
│  5. kernel_main(): C 代码执行                                 │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 最小内核目标

实现一个「最小可运行内核」：
- 能够在 QEMU 中启动
- 具备串口输出能力
- 运行在 64 位长模式
- 能够打印 "Welcome to AntX!"

### 3.3 核心组件

| 组件 | 说明 | 阶段 |
|------|------|------|
| boot.asm | Multiboot2 头 + 长模式切换 | 初期 |
| main.c | 内核入口，初始化串口 | 初期 |
| serial.c | 串口驱动 | 初期 |
| gdt.c | 全局描述符表 | 初期 |
| idt.c | 中断描述符表 | 初期 |

## 四、构建系统

### 4.1 Makefile 配置

```makefile
CC = x86_64-linux-gnu-gcc
LD = x86_64-linux-gnu-ld
AS = nasm

CFLAGS = -std=c11 -m64 -Wall -Wextra -nostdinc -nostdlib -fno-pie -fno-stack-protector \
         -fno-asynchronous-unwind-tables -fno-ident -mcmodel=kernel \
         -Isrc/include

LDFLAGS = -T src/link.ld -nostdlib

ASFLAGS = -f elf64

KERNEL_OBJS = build/boot.o build/main.o build/serial.o

LOG_DIR = logs

.PHONY: all clean run debug log iso run-iso

all: build/kernel.bin

build/kernel.bin: $(KERNEL_OBJS)
	$(LD) $(LDFLAGS) -o $@ $(KERNEL_OBJS)

build/%.o: src/kernel/%.c
	@mkdir -p build
	$(CC) $(CFLAGS) -c $< -o $@

build/%.o: src/kernel/%.asm
	@mkdir -p build
	$(AS) $(ASFLAGS) $< -o $@

iso: all
	@mkdir -p isodir/boot/grub
	cp build/kernel.bin isodir/boot/kernel.bin
	echo 'set timeout=0' > isodir/boot/grub/grub.cfg
	echo 'set default=0' >> isodir/boot/grub/grub.cfg
	echo '' >> isodir/boot/grub/grub.cfg
	echo 'menuentry "AntX" {' >> isodir/boot/grub/grub.cfg
	echo '    multiboot2 /boot/kernel.bin' >> isodir/boot/grub/grub.cfg
	echo '}' >> isodir/boot/grub/grub.cfg
	grub2-mkrescue -o build/antx.iso isodir

clean:
	rm -rf build/ isodir/

run-iso: iso
	qemu-system-x86_64 -cdrom build/antx.iso -serial stdio

debug: all
	qemu-system-x86_64 -kernel build/kernel.bin -serial stdio -s -S

log: all
	@mkdir -p $(LOG_DIR)
	qemu-system-x86_64 -kernel build/kernel.bin -serial file:$(LOG_DIR)/serial.log -display none
	@echo "Serial log saved to $(LOG_DIR)/serial.log"
```

### 4.2 链接脚本 (link.ld)

```ld
OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

SECTIONS
{
    . = 0x100000;
    
    .multiboot2 : {
        *(.multiboot2)
    }
    
    .text : {
        *(.text)
        *(.text.*)
    }
    
    .rodata : {
        *(.rodata)
        *(.rodata.*)
    }
    
    .data : {
        *(.data)
        *(.data.*)
    }
    
    .bss : {
        *(.bss)
        *(.bss.*)
        *(COMMON)
    }
    
    /DISCARD/ : {
        *(.comment)
        *(.note.*)
        *(.eh_frame*)
    }
}
```

### 4.3 构建命令

```bash
# 构建内核
make all

# 创建可启动 ISO
make iso

# 通过 ISO 运行 (推荐)
make run-iso

# 调试模式
make debug

# 保存日志到文件
make log

# 清理构建目录
make clean
```

## 五、引导代码说明

### 5.1 boot.asm 结构

```asm
BITS 32

section .multiboot2
align 8
; Multiboot2 头 - 让 GRUB 能够识别内核

section .bss
align 4096
; 页表和栈空间

section .rodata
align 16
; GDT 表

section .text
global _start
extern kernel_main

_start:
    ; 1. 初始化页表 (PML4 → PDPT → PD)
    ; 2. 设置 CR3 指向 PML4
    ; 3. 启用 PAE (CR4.PAE)
    ; 4. 启用长模式 (MSR)
    ; 5. 启用分页 (CR0.PG)
    ; 6. 加载 GDT
    ; 7. 跳转到 64 位代码

BITS 64
long_mode_start:
    ; 8. 设置段寄存器
    ; 9. 设置栈指针
    ; 10. 调用 kernel_main()
```

### 5.2 关键技术点

| 技术点 | 说明 |
|--------|------|
| Multiboot2 | GRUB 引导协议，支持 64 位内核 |
| 页表设置 | PML4 → PDPT → PD 三级映射 |
| PAE | 物理地址扩展，64 位寻址必需 |
| 长模式 | x86_64 的 64 位运行模式 |
| GDT | 全局描述符表，段选择子 |

## 六、开发流程

### 6.1 核心任务：实现中断等待 + 进程调度兼容的内核主循环

这是**真实操作系统内核的标准主循环**，也是 AntX 从"演示内核"变成"正式内核"的关键。

#### 为什么不能只用简单死循环？
| 方式 | 问题 |
|------|------|
| `while(1);` | 100% CPU 空转 |
| `while(1) hlt;` | 无法集成调度，结构不规范 |
| **interrupt_idle()** | 低功耗、可响应中断、可无缝接入进程调度 |

#### AntX 标准主循环模型
```c
// 内核主循环（工业标准写法）
void kernel_main(void) {
    // 基础初始化
    serial_init();
    gdt_init();
    idt_init();
    proc_init();

    serial_puts("[AntX] Kernel started successfully.\n");

    // 开启全局中断
    enable_interrupts();

    // 正式进入内核主循环
    while (1) {
        interrupt_idle();
    }
}
```

### 6.2 中断等待核心实现

#### include/kernel.h
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>

void enable_interrupts(void);
void disable_interrupts(void);
void interrupt_idle(void);
void kernel_main(void);
```

#### kernel/idt.c（核心）
```c
#include "idt.h"
#include "kernel.h"
#include "proc.h"
#include "io.h"

bool interrupt_wait_enabled = true;

// 中断等待 + 进程调度兼容
void interrupt_idle(void) {
    if (!interrupt_wait_enabled) return;

    // 有进程 → 调度
    if (proc_has_runnable()) {
        schedule();
        return;
    }

    // 无进程 → 休眠等待中断
    __asm__ volatile (
        "sti\n"
        "hlt\n"
        "cli\n"
        :::"memory"
    );
}

void enable_interrupts(void) {
    __asm__ volatile ("sti");
}

void disable_interrupts(void) {
    __asm__ volatile ("cli");
}
```

### 6.3 进程调度兼容层（极简但可扩展）

#### include/proc.h
```c
#pragma once
#include <stdint.h>
#include <stdbool.h>

typedef enum {
    PROC_UNUSED,
    PROC_RUNNABLE,
    PROC_RUNNING,
    PROC_SLEEPING
} proc_state_t;

typedef struct proc {
    uint32_t pid;
    proc_state_t state;
    uintptr_t esp;
    uintptr_t eip;
    char name[16];
} proc_t;

#define MAX_PROC 16
extern proc_t proc_table[MAX_PROC];
extern proc_t* current_proc;

void proc_init(void);
bool proc_has_runnable(void);
void schedule(void);
```

#### proc/schedule.c
```c
#include "proc.h"
#include "serial.h"

proc_t proc_table[MAX_PROC] = {0};
proc_t* current_proc = NULL;

void proc_init(void) {
    for (int i = 0; i < MAX_PROC; i++) {
        proc_table[i].state = PROC_UNUSED;
        proc_table[i].pid = 0;
    }
    serial_puts("[AntX] Process manager initialized.\n");
}

bool proc_has_runnable(void) {
    for (int i = 0; i < MAX_PROC; i++) {
        if (proc_table[i].state == PROC_RUNNABLE)
            return true;
    }
    return false;
}

void schedule(void) {
    if (!proc_has_runnable()) return;

    for (int i = 0; i < MAX_PROC; i++) {
        if (proc_table[i].state == PROC_RUNNABLE) {
            current_proc = &proc_table[i];
            current_proc->state = PROC_RUNNING;
            return;
        }
    }
}
```

### 6.4 实现顺序（最稳路线）

```
1. 串口输出
2. GDT
3. IDT & 中断重映射
4. 中断等待（interrupt_idle）
5. 时钟中断（PIT）
6. 键盘中断
7. 进程调度 & 上下文切换
8. 文件系统 / PWID / Shell
```

### 6.5 迭代开发

```
┌─────────────────────────────────────────────────────────────┐
│                    开发迭代循环                              │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│   编写代码 ──▶ 编译 ──▶ 运行测试 ──▶ 调试 ──▶ 修复 ──▶   │
│       ▲                                              │      │
│       │______________________________________________│      │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

### 6.6 测试方法

#### 方法1：ISO 启动 (推荐)

```bash
make iso
make run-iso
```

#### 方法2：QEMU + GDB 调试

终端1:
```bash
make debug
```

终端2:
```bash
gdb
(gdb) target remote localhost:1234
(gdb) break kernel_main
(gdb) continue
```

#### 方法3：串口输出调试

```bash
make log
cat logs/serial.log
```

### 6.7 添加功能顺序

| 阶段 | 添加功能 | 说明 |
|------|----------|------|
| 1 | 串口输出 | 最基础的调试手段 |
| 2 | GDT/IDT | 中断和段管理 |
| 3 | 内存管理 | 动态内存分配 |
| 4 | 进程调度 | 多任务支持 |
| 5 | PWID 模块 | 权限系统 |
| 6 | 文件系统 | 存储支持 |
| 7 | Shell | 命令行界面 |

## 七、常用调试技巧

### 7.1 串口打印

```c
void serial_putc(uint16_t port, char c) {
    while (!(inb(port + 5) & 0x20));
    outb(port, c);
}

void serial_puts(uint16_t port, const char *s) {
    while (*s) {
        if (*s == '\n') serial_putc(port, '\r');
        serial_putc(port, *s++);
    }
}
```

### 7.2 打印十六进制

```c
void serial_put_hex(uint16_t port, uint64_t val) {
    const char hex_chars[] = "0123456789ABCDEF";
    char buf[17];
    
    for (int i = 15; i >= 0; i--) {
        buf[i] = hex_chars[val & 0xF];
        val >>= 4;
    }
    buf[16] = '\0';
    
    serial_puts(port, "0x");
    serial_puts(port, buf);
}
```

### 7.3 内核崩溃处理

```c
void panic(const char *msg) {
    serial_puts(SERIAL_COM1, "\n\n!!! PANIC !!!\n");
    serial_puts(SERIAL_COM1, msg);
    serial_puts(SERIAL_COM1, "\nSystem halted.\n");
    while (1) __asm__ volatile ("hlt");
}
```

## 八、常见问题

### 8.1 QEMU 无法加载 64 位内核

**问题**: `Cannot load x86-64 image, give a 32bit one.`

**解决**: 使用 GRUB + ISO 方式启动，而不是 `qemu -kernel`:
```bash
make iso
make run-iso
```

### 8.2 GRUB 工具未安装

**问题**: `grub2-mkrescue: command not found`

**解决**: 安装 GRUB 工具包:
```bash
sudo dnf install grub2-tools xorriso
```

### 8.3 交叉编译器问题

**问题**: 编译时出现架构不匹配

**解决**: 使用交叉编译工具链:
```bash
sudo dnf install gcc-x86_64-linux-gnu binutils-x86_64-linux-gnu
```

### 8.4 GCC 版本兼容问题

**问题**: `error: 'bool' cannot be defined via 'typedef'`

**原因**: GCC 15 默认使用 C23 标准，`bool` 已成为关键字

**解决**: 在 Makefile 中指定 C11 标准:
```makefile
CFLAGS = -std=c11 -m64 -Wall -Wextra ...
```

## 九、参考资源

- OS Dev Wiki: https://wiki.osdev.org
- QEMU 文档: https://www.qemu.org/documentation/
- Intel x64 手册: Intel SDM
- Multiboot2 规范: https://www.gnu.org/software/grub/manual/multiboot2/

## 十、注意事项

1. **使用 ISO 启动** - 64 位内核需要 GRUB 引导
2. **从简单开始** - 不要一次性实现所有功能
3. **频繁测试** - 每次添加功能后都测试
4. **使用 GDB** - 复杂问题用调试器
5. **串口优先** - 串口调试是最可靠的方式
6. **保持耐心** - 内核开发需要时间积累
7. **日志管理** - 所有日志存放在 logs/ 目录

## 十一、规范总结（最重要）

1. **主循环必须用 interrupt_idle()**
2. **永远不要用空转死循环**
3. **先 sti，再 hlt，顺序不能反**
4. **进程调度在 idle 中自然接入，无需重构**
