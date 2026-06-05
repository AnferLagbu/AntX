# 架构移植指南 (Architecture Porting Guide)

本文将指导你如何为 AntX (QueenX) 内核添加新的 CPU 架构支持。
以 x86_64 为参考架构，aarch64 为首个移植目标。

---

## 目录

1. [架构抽象层 (Arch trait)](#1-架构抽象层)
2. [Phase 1: 类型定义与薄封装](#2-phase-1-类型定义)
3. [Phase 2-3: 真实实现与调用方迁移](#3-phase-2-3-实现与迁移)
4. [Phase 4: 构建系统多目标化](#4-phase-4-构建系统)
5. [Phase 5: 启动入口与 stub](#5-phase-5-启动入口)
6. [Phase 6: 完整实现](#6-phase-6-完整实现)
7. [Phase 7: 测试与文档](#7-phase-7-测试与文档)

---

## 1. 架构抽象层

### Arch trait (20 方法, 8 大类)

```
src/kernel/arch/mod.rs  → trait Arch
├── 中断:    interrupt_disable / interrupt_restore / interrupt_enable / is_interrupt_enabled
├── CPU 控制: halt
├── MMU:     tlb_flush_page / tlb_flush_all / read_page_table_base / write_page_table_base / read_fault_address
├── 上下文:  context_switch
├── 用户态:  enter_user / return_to_user
├── CPU 信息: cpu_id / timestamp
├── 屏障:    fence / fence_w
├── IPI:     send_ipi / broadcast_ipi
├── 端口IO:  outb / inb / outl / inl
└── 系统控制: shutdown / reboot
```

### 编译期多态

```rust
// 编译时选择
#[cfg(target_arch = "x86_64")]
type CurrentArch = X8664;
#[cfg(target_arch = "aarch64")]
type CurrentArch = Aarch64;

// 零开销调用
macro_rules! arch {
    ($method:ident $(, $arg:expr)*) => {
        <$crate::kernel::arch::CurrentArch as $crate::kernel::arch::Arch>::$method($($arg),*)
    };
}
```

### 薄封装层 (避免全部内核感知架构类型)

```
src/kernel/cpu/arch.rs     → cpu_id(), timestamp(), halt(), send_ipi(), broadcast_ipi()
src/kernel/sync/arch.rs    → spin_hint(), fence(), fence_w(), interrupt_save/restore()
src/kernel/mm/arch.rs      → tlb_flush_page(), tlb_flush_all(), read/write_page_table_base()
```

---

## 2. Phase 1: 类型定义

以添加 riscv64 为例:

### 2.1 创建架构模块

```bash
mkdir -p src/kernel/arch/riscv64
```

### 2.2 定义类型和 stub impl

`src/kernel/arch/riscv64/mod.rs`:
```rust
pub struct Riscv64;

impl super::Arch for Riscv64 {
    // 先用 unimplemented!() 填充 20 个方法
    // 参考 aarch64 stub: https://github.com/AnferLagbu/AntX/blob/feature/multiarch-phase1/src/kernel/arch/aarch64/mod.rs
}
```

### 2.3 注册到系统

`src/kernel/arch/mod.rs`:
```rust
#[cfg(target_arch = "riscv64")]
pub mod riscv64;

#[cfg(target_arch = "riscv64")]
type CurrentArch = riscv64::Riscv64;
```

### 2.4 验证

```bash
cargo build --target riscv64gc-unknown-none-elf
```

---

## 3. Phase 2-3: 实现与迁移

参考 x86_64 和 aarch64 实现:

| 架构 | 中断控制 | MMU | 上下文切换 | 时间戳 |
|------|----------|-----|-----------|--------|
| x86_64 | pushfq/cli/sti | CR3/invlpg | push/pop + iretq | rdtsc |
| aarch64 | DAIF (msr daifset/clr) | TTBR0/TLBI | str/ldr + eret | cntpct_el0 |
| riscv64 | sstatus/mstatus SIE | satp/sfence.vma | sd/ld + mret/sret | rdtime |

第 3 阶段大规模迁移调用方:
```bash
# 禁止的写法
asm!("cli");
asm!("rdtsc");

# 正确的写法
crate::arch!(interrupt_disable());
crate::arch!(timestamp());
```

---

## 4. Phase 4: 构建系统

### Makefile 添加架构支持

```makefile
ARCH ?= x86_64

ifeq ($(ARCH),riscv64)
    CC = riscv64-linux-gnu-gcc
    LD = riscv64-linux-gnu-ld
    RUST_TARGET = riscv64gc-unknown-none-elf
    QEMU = qemu-system-riscv64
    QEMU_MACHINE = virt
    LDSCRIPT = src/kernel/link/riscv64.ld
endif
```

### 链接脚本

创建 `src/kernel/link/riscv64.ld`（参考 `aarch64.ld`）。

### Cargo 配置

在 `src/rust/.cargo/config.toml` 添加:
```toml
[target.riscv64gc-unknown-none-elf]
rustflags = [
    "-C", "relocation-model=static",
    "-C", "link-arg=-nostdlib",
]
```

---

## 5. Phase 5: 启动入口

### 5.1 汇编入口 (boot/riscv64/start.S)

以 RISC-V QEMU virt 为例:
```
内核入口: 0x80000000
启动流程: M-mode → S-mode (或 OpenSBI → S-mode)
主要职责:
  1. 设置栈指针 (sp)
  2. 清除 BSS
  3. 跳转 Rust entry()
```

### 5.2 Rust 入口 (boot/riscv64/entry.rs)

参考 aarch64 入口流程:
```
BSS 清零 → UART 初始化 → MMU 初始化 → 异常向量设置 → 
中断控制器初始化 → 定时器初始化 → 跳转 kernel_init()
```

### 5.3 架构特定模块

| 模块 | 说明 | RISC-V 对应 |
|------|------|------------|
| mmu.rs | 页表管理 | Sv39/Sv48, satp, sfence.vma |
| exception.rs | 异常向量 | mtvec/stvec, CSR 保存/恢复 |
| timer.rs | 定时器 | mtime/mtimecmp (CLINT), Sstc 扩展 |
| uart.rs | 串口驱动 | ns16550a (QEMU virt 默认) |
| smp.rs | 多核启动 | IPI via CLINT/ACLINT |

---

## 6. Phase 6: 完整实现

### cfg 门控清单

所有 x86_64 专属代码必须添加 `#[cfg(target_arch = "x86_64")]`:

| 模块 | 文件 | 类型 |
|------|------|------|
| cpu | cpuid.rs, msr.rs | 子模块级 |
| smp | mod.rs | 模块级 |
| pci | mod.rs | 模块级 |
| syscall | mod.rs | 模块级 |
| idt | handlers.rs, idt.rs, safety.rs | 函数级 |
| dma | ffi.rs, engine.rs | 函数级 (lfence) |
| lib | string.rs | 函数级 (rep stosb) |

---

## 7. Phase 7: 测试与文档

### 验证清单

```bash
# 源架构 (x86_64) 编译
cargo build --release --target x86_64-unknown-none

# 新架构编译
cargo build --release --target <new-target>

# 主机测试
make test-host

# 禁止模式检查 (确保无直接 asm)
grep -rn 'asm!("cli")' src/kernel/
grep -rn 'asm!("rdtsc")' src/kernel/
```

---

## 参考资源

- [ARM Architecture Reference Manual (ARMv8-A)](https://developer.arm.com/documentation/ddi0487/)
- [RISC-V Privileged Specification](https://github.com/riscv/riscv-isa-manual)
- [QEMU virt machine documentation](https://www.qemu.org/docs/master/system/arm/virt.html)
- AntX 多架构解耦规划书: `docs/development/multiarch-decoupling-plan.md`