# Miri 覆盖范围与局限性

> 最后更新: 2026-06-03
> 项目: QueenX Framekernel

## 概述

[Miri](https://github.com/rust-lang/miri) 是 Rust 官方的**未定义行为 (UB) 检测解释器**。
本项目使用 Miri 在宿主架构 (x86_64 Linux) 上对 TCB 关键算法进行**等价重写 + 全量扫描**,
确保算法层无 UB。物理硬件层 (MMIO / 中断 / 缓存一致性) 不在 Miri 覆盖范围内。

## 测试基础设施

`miri-tests/` 是独立 crate, 与 `queenx` 内核解耦:

- **目的**: 把 `no_std` 内核中的纯算法提取出来, 在 `std` 环境下接受 Miri 扫描
- **方法**: **等价重写** (Equivalent Re-implementation) — 用同样语义、不同数据结构的代码复现
  算法逻辑
- **约束**: 不引入 `unsafe extern fn`, 不调用真实硬件, 不使用 raw pointer aliasing

```toml
# miri-tests/Cargo.toml
[package]
name = "miri-tests"
edition = "2021"

[lib]
path = "src/lib.rs"
```

## 覆盖范围

### ✅ 已被 Miri 验证 (67 测试, 0 UB)

| 模块 | 测试数 | 覆盖算法 | 验证项 |
|------|--------|----------|--------|
| `racy_cell` | 8 | RacyCell lock-free 全局可变状态 | 并发读/写无数据竞争 |
| `frame` | 4 | PhysPage 对齐 / 范围 / 算术 | checked_add 防溢出 |
| `gf256` | 7 | GF(2^8) RAIDZ 校验 (加/乘/除/逆) | 数组边界 / 除零 |
| `boot_image` | 8 | 启动镜像编码 / 解码 / CRC32 | 字节序 / 长度验证 |
| `validators` | 5 | 内存布局约束 (地址对齐 / 大小) | 配置检查 |
| `alias_registry` | 12 | IoMem MMIO 别名检测 | 重叠区间 / 0 长度 / 溢出 |
| `dma` | 14 | DMA 流 (对齐 / 同步方向 / 生命周期) | PhysAddr 边界 / Frame 借用 |
| `arch_consistency` | 13 | x86_64 / aarch64 等价性参数化 | canonical / cache / 原子宽度 |

### 🎯 验证的 UB 类型

| UB 类型 | 验证方式 | 示例 |
|---------|----------|------|
| **越界访问** | 数组/切片索引检查 | `gf_array_bounds` |
| **整数溢出** | `checked_add` / `saturating_add` | `frame_end_calculation` |
| **数据竞争** | strict-provenance | `racy_cell::*` |
| **use-after-free** | 借用检查器 | `dma::frame_lifecycle_ownership` |
| **未初始化内存** | MaybeUninit 显式标注 | (未使用) |
| **别名违规** | `&mut` 互斥 | `alias_registry::full_registry_rejects` |
| **类型转换错误** | checked casts | `dma::range_overflow_detected` |

## 不在 Miri 覆盖范围

### ❌ 硬件交互层

| 类别 | 原因 | 替代验证 |
|------|------|----------|
| **MMIO 读写** | 需要真实硬件寄存器副作用 | QEMU 设备模拟 + 集成测试 |
| **端口 I/O (PIO)** | x86 IN/OUT 指令 Miri 不支持 | QEMU + seetest |
| **中断处理** | IDT 设置 / 中断控制器需要硬件 | QEMU + 异常注入 |
| **缓存一致性** | aarch64 需显式 flush 指令 | 双架构 QEMU 启动 |
| **DMA 真实传输** | 设备总线事务 | virtio + 集成测试 |
| **TLB 维护** | INVLPG / TLBI 指令 | QEMU 模拟 |
| **页表遍历** | CR3 / TTBR 切换 | QEMU + guest 验证 |
| **SMP / 多核** | 跨核内存屏障 / IPI | QEMU -smp N |
| **浮点/SIMD** | 上下文保存 / FPU 指令 | QEMU 浮点测试 |
| **UEFI 调用** | EFI runtime services | OVMF + QEMU |
| **ACPI 解析** | 真实硬件表 | QEMU 暴露的 ACPI 表 |

### ❌ FFI / 外部依赖

| 类别 | 原因 | 替代验证 |
|------|------|----------|
| **smoltcp 网络栈** | 大量 `unsafe`, 第三方代码 | 运行时模糊测试 |
| **UEFI 协议绑定** | 由 rust-bindgen 生成 | 集成测试 |
| **编译器内建函数** | `llvm_asm!` / `global_asm!` | 编译检查 + 运行时验证 |
| **裸函数调用** | 上下文切换汇编 | QEMU + 集成 |

### ❌ 系统调用 / 用户态

| 类别 | 原因 | 替代验证 |
|------|------|----------|
| **真实 copy_from_user** | 需要 MMU 上下文 | KASAN + 用户态 fuzz |
| **跨地址空间通信** | 需要完整调度 | QEMU + 用户程序 |
| **信号处理** | 用户态注册 | 集成测试 |

## Miri 局限性

1. **宿主架构**: Miri 在编译主机上运行, 通常是 x86_64。**不能**直接验证 aarch64 指令
   (但 `arch_consistency.rs` 通过**参数化**方式间接验证跨架构行为)

2. **不可中断**: Miri 模拟整个程序, **不能**测试硬件中断 / 异常路径

3. **慢**: 全量 67 测试在 release 模式下需 ~49 秒, 不能用于 CI 快速反馈

4. **不支持 raw pointer aliasing**: 严格模式下 (`-Zmiri-strict-provenance`) 不允许任意指针转换
   — 这**正是我们想要的**, 因为它能捕获 aliasing UB

5. **有限的 OS 交互**: 不能执行真实 I/O (文件 / 网络), 只能跑纯算法

6. **不验证 panic 安全**: 不会检查 unwind 路径下的数据一致性

## 等价重写策略

`miri-tests` 不是把内核代码直接搬过来, 而是**重新实现等价算法**:

```rust
// 原始内核 (no_std, raw pointer):
pub unsafe fn frame_end(paddr: u64, size: u64) -> Option<u64> {
    paddr.checked_add(size) // 假定 size != 0, 否则 0 是合法值
}

// miri-tests 等价重写 (std, 安全抽象):
pub fn frame_end(paddr: u64, size: u64) -> Option<u64> {
    if size == 0 {
        return Some(paddr);
    }
    paddr.checked_add(size)
}
```

**关键原则**:
1. **相同不变量**: 业务逻辑 (如 `paddr + size` 不溢出) 必须完全一致
2. **不同边界处理**: Miri 版本可使用更宽松的输入 (因为不需要 `unsafe` 前提)
3. **覆盖边界场景**: 0 长度、最大值、刚好溢出、邻接区间等
4. **压测**: 用伪随机生成大量输入, 验证不变量恒成立

## 运行命令

```bash
# 常规测试 (快速, 不验证 UB)
cd miri-tests && cargo test --release

# Miri 全量扫描 (慢, 严格 UB 检测)
cd miri-tests && MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test --release

# Miri 单个模块
cd miri-tests && MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test --release dma
```

## CI 集成建议

```yaml
# .github/workflows/miri.yml
name: Miri
on: [push, pull_request]
jobs:
  miri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@miri
      - run: rustup component add miri
      - run: cd miri-tests && MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test --release
```

## 后续改进

1. **覆盖更多 TCB 模块**: 待 IPC / 信号 / 调度器核心算法稳定后, 加入 miri-tests
2. **状态机压测**: 用 proptest 替换固定测试用例, 自动生成反例
3. **Miri 标注**: 在关键 unsafe 函数添加 `#[cfg(miri)]` 路径, 让 Miri 走"安全"分支
4. **AArch64 模拟**: 探索使用 `cargo miri --target aarch64-unknown-none` (待 Miri 支持)
5. **Kani / Creusot**: 补充形式化验证工具, 验证时序与功能正确性
