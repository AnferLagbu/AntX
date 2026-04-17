# 用户态进程启动 GPF 问题

## 问题概述

**问题类型**: 内核态到用户态切换失败
**严重程度**: 高
**状态**: 调试中
**日期**: 2026-04-13

## 问题描述

系统在从内核态切换到用户态执行第一个用户进程时，发生 **General Protection Fault (GPF, exception 0x0D)**，错误码为 0，发生在用户代码入口点 `0x400D28`。

## 系统架构背景

- **双映射启动方案**：内核同时具有恒等映射 (0x0-0x...) 和高地址映射 (0xFFFF800001000000+)
- **目标**：通过 `iretq` 指令从内核态 (CPL=0) 切换到用户态 (CPL=3) 执行 init 进程

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

## 用户代码入口指令

```asm
0000000000400d28 <_start>:
  400d28:  55                   push   %rbp        ; <- GPF 在此
  400d29:  48 89 e5             mov    %rsp,%rbp
  400d2c:  66 b8 23 00          mov    $0x23,%ax
  400d30:  8e d8                mov    %eax,%ds
  ...
```

## iretq 栈帧内容 (从低到高)

```
[SS=0x23]     <- RSP 指向此处
[RSP=0x7FFFFFFFFFFF000]
[RFLAGS=0x202]
[CS=0x1B]
[RIP=0x400D28]
```

## 已尝试的修复方案

| 日期 | 修复项 | 文件 | 说明 |
|------|--------|------|------|
| 2026-04-13 | USER_STACK_TOP 规范地址 | src/include/user_proc.h | `0x7FFFFFFFFFFF000` → `0x00007FFFFFFFFFF0ULL` |
| 2026-04-13 | TSS 描述符 64 位 | src/kernel/gdt.c | 高 32 位地址正确设置 |
| 2026-04-13 | iretq 前 DS/ES | src/proc/scheduler.c | 设置为 0x23 (用户数据段) |
| 2026-04-13 | 禁用 stack canary | Makefile | `-fno-stack-protector` |
| 2026-04-13 | kernel_main 地址 | src/kernel/boot.asm | 硬编码地址与实际地址匹配 |
| 2026-04-13 | 栈地址高地址 | src/kernel/boot.asm | `0xFFFF8000011701e` |
| 2026-04-13 | invlpg 语法 | src/kernel/boot.asm | 使用寄存器间接寻址 |
| 2026-04-13 | retfq 语法 | src/kernel/gdt.asm | 修复汇编语法错误 |

## 关键疑问

1. **GPF error code=0** 表示不是段选择子问题，那具体是什么原因？
2. **DS/ES=0x0000** 在 x86-64 长模式下是否真的允许？是否需要在 iretq 前设置？
3. 用户代码页的 **PTE 中 RW=0**（只读），执行 `push %rbp` 写栈是否应该正常工作？（栈是单独映射的，RW=1）
4. 是否与 **双映射架构** 有关？内核的高地址映射是否影响了用户态切换？

## QEMU 调试日志

```
check_exception old: 0xffffffff new 0xd
     5: v=0d e=0000 i=0 cpl=3 IP=001b:0000000000400d28 pc=0000000000400d28
RIP=0000000000400d28 RFL=00000202 [-------] CPL=3 II=0 A20=1 SMM=0 HLT=0
ES =0000 0000000000000000 ffffffff 00cf1300
CS =001b 0000000000000000 ffffffff 00affa00 DPL=3 CS64 [-R-]
SS =0023 0000000000000000 ffffffff 00cff200 DPL=3 DS   [-W-]
DS =0000 0000000000000000 ffffffff 00cf1300
CR3=0000000000240000 CR4=00000020
EFER=0000000000000500
```

## 相关文件

| 文件 | 说明 |
|------|------|
| [src/proc/scheduler.c](file:///home/anfer/Code/C/AntX/src/proc/scheduler.c) | iretq 内联汇编 |
| [src/proc/switch.asm](file:///home/anfer/Code/C/AntX/src/proc/switch.asm) | process_start_user_asm |
| [src/mm/vmm.c](file:///home/anfer/Code/C/AntX/src/mm/vmm.c) | vmm_create_user_page_table |
| [src/kernel/gdt.c](file:///home/anfer/Code/C/AntX/src/kernel/gdt.c) | GDT 初始化 |
| [src/kernel/boot.asm](file:///home/anfer/Code/C/AntX/src/kernel/boot.asm) | 高地址跳转代码 |
| [src/kernel/gdt.asm](file:///home/anfer/Code/C/AntX/src/kernel/gdt.asm) | gdt_flush |

## 调试日志位置

- `logs/qemu_debug28.log` - QEMU 异常日志
- `logs/serial.log` - 内核串口输出

## 待请教的问题

1. x86-64 长模式下，`iretq` 从内核态切换到用户态时，DS/ES/FS/GS 段寄存器的正确处理方式是什么？

2. GPF error code=0 在用户态代码执行第一条指令时发生，可能的原因有哪些？

3. 用户页表需要映射哪些必要的内容才能保证 `iretq` 后正常执行？
