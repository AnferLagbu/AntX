# 用户态进程启动 GPF 问题

## 问题概述

**问题类型**: 内核启动失败 → 内核启动成功
**严重程度**: 高
**状态**: 已修复
**日期**: 2026-04-18

## 问题历史

### 阶段 1: 内核启动 GPF (已修复)

**问题描述**: 内核在启动过程中发生 GPF，无法进入 kernel_main。

**根本原因**: 
1. **栈指针地址错误**: boot.asm 中的栈指针地址 `0xFFFF8000011701e` 映射到物理地址 `0x1701e`，而不是预期的 `0x11701e`。地址中少了一个 `1`。

**修复方案**:
```asm
# 旧值 (错误)
mov rsp, qword 0xFFFF8000011701e

# 新值 (正确)
mov rsp, qword 0xFFFF80000111701e
```

**验证结果**: 内核成功启动，输出 `HIGH\NOCPY\CR3\STK\CALL\`

### 阶段 2: 用户态切换 GPF (待修复)

**问题描述**: 系统在从内核态切换到用户态执行第一个用户进程时，发生 **General Protection Fault (GPF, exception 0x0D)**。

## 已验证正确的部分

| 项目 | 状态 | 验证值 |
|------|------|--------|
| 用户代码页表映射 | ✅ | PTE: `P=1, RW=0, US=1, XD=0` |
| 用户栈映射 | ✅ | 物理地址 0x251FF0 可访问 |
| 内核栈映射到用户页表 | ✅ | 0x116000 页已映射 |
| CR3 切换 | ✅ | CR3=0x240000 (用户页表) |
| 段选择子 | ✅ | CS=0x1B, SS=0x23 (DPL=3) |
| RSP 对齐 | ✅ | 16字节对齐 |
| RFLAGS | ✅ | 0x202 (IF=1) |

## 异常发生时的 CPU 状态

```
RIP=0x400D28  (用户代码入口: push %rbp)
CPL=3         (已成功切换到用户态)
CS=0x1B       (用户代码段, DPL=3, CS64)
SS=0x23       (用户数据段, DPL=3)
DS=ES=FS=GS=0x0000  (NULL - 问题点)
CR3=0x240000  (用户页表)
Exception: GPF (v=0d), Error Code=0
```

## 关键发现

### 1. DS/ES 段寄存器问题

在 x86-64 长模式下，虽然段基址被忽略，但 **当 CPL=3（用户态）时，数据段选择子不能为 NULL**。

**解决方案**: 
- 在 `iretq` 之前设置 DS/ES/FS/GS = 0x23
- 在用户程序入口 (`_start`) 使用 `__attribute__((naked))` 并立即设置段寄存器

### 2. 用户程序入口问题

编译器会在函数开头生成序言代码（如 `push %rbp`），这会在设置 DS/ES 之前执行，导致 GPF。

**解决方案**: 使用 `__attribute__((naked))` 阻止编译器生成序言：

```c
__attribute__((naked)) void _start(void) {
    __asm__ volatile(
        "mov $0x23, %%ax\n"
        "mov %%ax, %%ds\n"
        "mov %%ax, %%es\n"
        "mov %%ax, %%fs\n"
        "mov %%ax, %%gs\n"
        "xor %%rbp, %%rbp\n"
        "call main\n"
        "mov $2, %%rax\n"  // sys_exit
        "xor %%rdi, %%rdi\n"
        "int $0x80\n"
        "1: hlt\n"
        "jmp 1b\n"
        : : : "ax", "memory"
    );
}
```

### 3. 成熟操作系统参考

**Linux 方案**:
1. 使用 `swapgs` 指令交换 GS 基址
2. 在 `entry_trampoline` 中设置段寄存器
3. `iretq` 前通过汇编代码设置 DS/ES/FS/GS

**FreeBSD 方案**:
1. 使用 trampoline 代码
2. 在 `iretq` 前设置段寄存器
3. 使用 `fxsave/fxrstor` 保存浮点状态

**共同点**:
1. `iretq` 前必须设置 DS/ES/FS/GS
2. 使用汇编 trampoline 代码
3. 设置正确的段选择子 (0x23 for user data)
4. 确保 GDT 中有正确的用户段描述符

## 分析脚本

创建了以下 Python 分析脚本辅助调试：

| 脚本 | 说明 |
|------|------|
| `scripts/analyze_kernel.py` | 分析内核镜像布局 |
| `scripts/verify_mapping.py` | 验证页表映射 |
| `scripts/detailed_mapping.py` | 详细页表映射分析 |
| `scripts/verify_stack.py` | 栈地址验证 |

## 相关文件

| 文件 | 说明 |
|------|------|
| [src/proc/scheduler.c](file:///home/anfer/Code/C/AntX/src/proc/scheduler.c) | iretq 内联汇编 |
| [src/proc/switch.asm](file:///home/anfer/Code/C/AntX/src/proc/switch.asm) | process_start_user_asm |
| [src/mm/vmm.c](file:///home/anfer/Code/C/AntX/src/mm/vmm.c) | vmm_create_user_page_table |
| [src/kernel/gdt.c](file:///home/anfer/Code/C/AntX/src/kernel/gdt.c) | GDT 初始化 |
| [src/kernel/boot.asm](file:///home/anfer/Code/C/AntX/src/kernel/boot.asm) | 高地址跳转代码 |
| [src/user/init/main.c](file:///home/anfer/Code/C/AntX/src/user/init/main.c) | 用户程序入口 |

## 下一步工作

1. 验证 kernel_main 是否正确执行
2. 修复用户态切换问题
3. 完善用户程序入口代码
