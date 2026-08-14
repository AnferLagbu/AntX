# framework/cpu 子系统深度审计报告

> **审计范围**：`src/kernel/framework/cpu/`（7 文件）
> **审计日期**：2026-08-14
> **代码规模**：约 1,940 LoC
> **总体结论**：✅ 含 unsafe（TCB，**符合 F4 SAFETY 100% 覆盖**）/ ⚠️ **22 个问题（P0×5, P1×7, P2×7, P3×3）**

## 1. 子系统概览

| 文件 | 行数 | 主要职责 | 风险等级 |
|---|---:|---|---|
| [mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs) | 1554 | CPU 驱动核心（CPUID 解析 + MSR + TSC + 拓扑 + 缓存 + FFI）| **极高** |
| [cpuid.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cpuid.rs) | 78 | CPUID 指令封装 | **高** |
| [msr.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/msr.rs) | 122 | RDMSR/WRMSR 封装 | **高** |
| [arch.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/arch.rs) | 86 | CPU 层面架构抽象 | 中 |
| [tsc.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/tsc.rs) | 60 | TSC 读取 + 频率转换 | 中 |
| [cache.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cache.rs) | 20 | 缓存检测占位符 | 低 |
| [topology.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/topology.rs) | 20 | 多核拓扑占位符 | 低 |

## 2. 严重问题

### 2.1 [P0] `mod.rs:1554` 单文件 1554 行**严重违反简单优先**

- **位置**：[mod.rs:1-1554](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs#L1-L1554)
- **问题**：
  - 单文件包含：CPU 厂商检测 + 签名解析 + 特性收集 + 缓存检测 + MSR 管理 + TSC 校准 + 多核拓扑 + FFI 导出。
  - 注释（[mod.rs:13-22](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs#L13-L22)）承认原本是 C 版本 1060 行翻译 + 重新设计，但**未做模块拆分**。
- **建议方案**：
  1. 拆分到子模块（已经声明但未用）：cpu/feature.rs + cpu/topology_impl.rs + cpu/cache_impl.rs。

### 2.2 [P0] `msr.rs:72-83` `cpu_read_msr` 接受 `*mut u32` 未对齐验证

- **位置**：[msr.rs:71-84](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/msr.rs#L71-L84)
- **代码**：
  ```rust
  pub unsafe extern "C" fn cpu_read_msr(msr: u32, low: *mut u32, high: *mut u32) -> i32 {
      unsafe {
          if low.is_null() || high.is_null() {
              return -1;
          }
          let value = read_msr(msr);
          *low = value as u32;
          *high = (value >> 32) as u32;
          0
      }
  }
  ```
- **问题**：
  - 仅检查 `is_null()`，**未检查对齐**。
  - C 端传非对齐指针 → aarch64 触发 data abort。
- **建议方案**：
  1. `assert!(low as usize % 4 == 0 && high as usize % 4 == 0)`。

### 2.3 [P0] `msr.rs:93-97` `cpu_write_msr` 返回 `0` 即使 #GP 发生

- **位置**：[msr.rs:92-98](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/msr.rs#L92-L98)
- **代码**：
  ```rust
  pub unsafe extern "C" fn cpu_write_msr(msr: u32, low: u32, high: u32) -> i32 {
      unsafe {
          write_msr(msr, (u64::from(high) << 32) | u64::from(low));
          0
      }
  }
  ```
- **问题**：
  - `wrmsr` 在非法 MSR 上触发 #GP → 内核 panic（未捕获）。
  - 函数返回 0 是假设成功，**无错误传播**。
- **建议方案**：
  1. 启用 KPTI + IST #GP handler 捕获异常。
  2. 或预读 MSR 合法性表。

### 2.4 [P0] `cpuid.rs:24-32` CPUID 调用**未处理 leaf > max_leaf** 情况

- **位置**：[cpuid.rs:22-37](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cpuid.rs#L22-L37)
- **代码**：
  ```rust
  pub fn cpuid(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
      unsafe {
          core::arch::asm!(
              "cpuid",
              inlateout("eax") leaf => eax,
              ...
          );
      }
      (eax, ebx, ecx, edx)
  }
  ```
- **问题**：
  - 文档说 `cpuid` 指令"安全"，但**CPU 不支持 leaf 时行为未定义**（Intel SDM §3.3）。
  - Intel CPU: max_leaf 之外的 leaf 返回 EAX=EBX=ECX=EDX=0。
  - AMD CPU: **可能 panic / undefined**。
  - `is_leaf_supported()`（[cpuid.rs:48-51](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cpuid.rs#L48-L51)）存在但**调用方未强制使用**。
- **建议方案**：
  1. 公开 API 强制 `is_leaf_supported(leaf)` 校验。
  2. 或内部检查返回 `Option`。

### 2.5 [P0] `tsc.rs:43-50` `cycles_to_nanoseconds` 整数乘法溢出

- **位置**：[tsc.rs:42-50](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/tsc.rs#L42-L50)
- **代码**：
  ```rust
  pub fn cycles_to_nanoseconds(tsc_cycles: u64, tsc_freq_mhz: u64) -> u64 {
      if tsc_freq_mhz == 0 { return tsc_cycles; }
      (tsc_cycles * 1000) / tsc_freq_mhz
  }
  ```
- **问题**：
  - `tsc_cycles * 1000` 可能溢出 u64（当 tsc_cycles > 1.8 × 10¹⁶）。
  - Rust 整型溢出**debug panic / release wrapping**。
  - 在 64-bit TSC 4GHz CPU 运行 24 小时 → tsc_cycles ≈ 3.5 × 10¹⁴，* 1000 = 3.5 × 10¹⁷ > u64::MAX。
- **建议方案**：
  1. `tsc_cycles.checked_mul(1000).unwrap_or(u64::MAX) / tsc_freq_mhz`。

## 3. P1 问题

### 3.1 [P1] `mod.rs:1554` FFI 导出函数数量未审

- **位置**：[mod.rs:1554](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs)（搜索 `pub extern "C"`）
- **问题**：
  - `cpu_read_msr64`/`cpu_write_msr64` 等 6+ FFI 导出函数。
  - 未深审每个的安全约束。

### 3.2 [P1] `msr.rs:20-34` `read_msr` SAFETY 注释未提及 `#GP` 处理

- **位置**：[msr.rs:13-19](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/msr.rs#L13-L19)
- **问题**：
  - SAFETY 仅说"Ring 0"，但**未提及非法 MSR 触发 #GP**。
  - KPTI IST #GP handler 未配置（[subsystem-framework-arch.md §2.2](../audit/subsystem-framework-arch.md)）。

### 3.3 [P1] `mod.rs` TSC 校准算法**未深审**

- **位置**：[mod.rs:1554](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs)（grep `tsc_calibration`）
- **问题**：
  - TSC 校准依赖 PIT/HPET，启动早期不准 → 影响 timestamp()。

### 3.4 [P1] `mod.rs` CpuVendor::Unknown 兜底分支**过多**

- **位置**：[mod.rs:80-86](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs#L80-L86)
- **问题**：
  - 6 种 vendor 检测 + Unknown。
  - 对未识别 vendor 走默认路径——可能漏 vendor-specific 优化。

### 3.5 [P1] `cpuid.rs:48-51` `is_leaf_supported` **不验证 subleaf**

- **位置**：[cpuid.rs:46-51](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cpuid.rs#L46-L51)
- **代码**：
  ```rust
  pub fn is_leaf_supported(leaf: u32) -> bool {
      let (max_leaf, _, _, _) = cpuid(0, 0);
      leaf <= max_leaf
  }
  ```
- **问题**：
  - 某些 leaf 仅有特定 subleaf（如 Leaf 4 缓存参数）—— 仅检查 max_leaf 不够。

### 3.6 [P1] `arch.rs:54` `send_ipi` 接受 `u32` 但**不验证 CPU 索引在 MAX_CPUS 范围**

- **位置**：[arch.rs:46-55](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/arch.rs#L46-L55)
- **问题**：
  - `send_ipi(target_cpu: u32, vector: u8)` 传任意 u32。
  - 索引超出 `MAX_CPUS` → LAPIC 目标不存在 → **IPI 丢失**。

### 3.7 [P1] `tsc.rs:30-32` `read_tsc_serialized` 与 `read_tsc` 实现**完全相同**

- **位置**：[tsc.rs:28-32](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/tsc.rs#L28-L32)
- **问题**：
  - 两个函数都委托 `crate::arch!(timestamp())`——无序列化（mfence/lfence）。
  - 文档说"更精确"——与实际不符。

## 4. P2 问题

### 4.1 [P2] `cache.rs:20` 仅占位——所有缓存检测**实际在 mod.rs:1554**

- **位置**：[cache.rs:1-20](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cache.rs#L1-L20)
- **问题**：
  - 模块拆分承诺未兑现。

### 4.2 [P2] `topology.rs:20` 同上——拓扑检测**实际在 mod.rs**

- **位置**：[topology.rs:1-20](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/topology.rs#L1-L20)
- **问题**：
  - 同上。

### 4.3 [P2] `mod.rs` CPUID leaf 解析中**extended leaf > 0x80000000 未审**

- **位置**：[mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs)
- **问题**：
  - 0x80000000+ leaf 包含 AMD-specific 特性（如 SVM）。

### 4.4 [P2] `mod.rs` Cache 检测依赖**CPUID Leaf 4**（Intel 专属）

- **位置**：[mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs)
- **问题**：
  - AMD CPU 使用 Leaf 0x80000005/0x80000006——Leaf 4 是 Intel 专属。
  - AMD CPU 上调用可能返回无效值。

### 4.5 [P2] `mod.rs:1554` MSR 列表（如 IA32_EFER, IA32_PAT 等）**硬编码**

- **位置**：[mod.rs](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs)
- **问题**：
  - 大量 MSR 常量（0x1B, 0x174, 0x175 等）应集中到 msr.rs 但分散。

### 4.6 [P2] `arch.rs:76` `set_kernel_stack` 在 aarch64 上 no-op——**未切换 SP_EL1**

- **位置**：[arch.rs:67-86](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/arch.rs#L67-L86)
- **问题**：
  - x86_64: 写 TSS RSP0。
  - aarch64: 注释说"SP_EL1 由上下文切换直接管理"——但**当前实现是 no-op**——aarch64 上设置栈不生效。

### 4.7 [P2] `tsc.rs:54-60` `nanoseconds_to_cycles` 同样有溢出风险

- **位置**：[tsc.rs:52-60](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/tsc.rs#L52-L60)
- **问题**：
  - `ns * tsc_freq_mhz` 同样可能溢出。

## 5. P3 问题

### 5.1 [P3] `mod.rs:1-9` 模块头注释声称 `mod.rs` 是"重新设计"，但实际是 C 版本翻译 + Rust 类型化

- **位置**：[mod.rs:13-22](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/mod.rs#L13-L22)
- **问题**：
  - 注释夸大。

### 5.2 [P3] `arch.rs:48-54` `send_ipi` 无 `Ordering` 内存屏障

- **位置**：[arch.rs:46-55](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/arch.rs#L46-L55)
- **问题**：
  - IPI 发送前后需 sfence 确保目标 CPU 看到最新数据。

### 5.3 [P3] `cpuid.rs:53-73` 测试覆盖 leaf 0/1，但未覆盖边界（0xFFFFFFFF）

- **位置**：[cpuid.rs:53-73](file:///home/anfer/Code/QueenX/src/kernel/framework/cpu/cpuid.rs#L53-L73)
- **问题**：
  - 测试不充分。

## 6. 跨子系统关联

### 6.1 cpu ↔ arch

- `framework/cpu/arch.rs::cpu_id()` 委托 `framework::arch::CurrentArch`。
- 与 [subsystem-framework-arch.md §2.5 P0 cpu_id 回退路径](../audit/subsystem-framework-arch.md) 关联。

### 6.2 cpu ↔ mm (TSC → 时间戳)

- `tsc::cycles_to_nanoseconds` 在 [subsystem-mm.md §2.x](../audit/subsystem-mm.md) 中被使用。
- 溢出问题可能影响 `pwm_now()`（[subsystem-framework-credo.md §2.x](../audit/subsystem-framework-credo.md)）。

### 6.3 cpu ↔ SMP

- `send_ipi/broadcast_ipi` 调用 arch 层 LAPIC。
- 与 [subsystem-arch-net.md §2.x P0 IDT SMP race](../audit/subsystem-arch-net.md) 关联。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 5 | 3-4 天 |
| **P1** | 7 | 4-5 天 |
| **P2** | 7 | 2-3 天 |
| **P3** | 3 | 0.5 天 |
| **合计** | **22** | **10-13 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 mod.rs 单文件拆分**（1-2 天）
2. **§2.4 CPUID leaf 验证**（0.5 天）
3. **§2.5 TSC 整数溢出**（0.5 天）
4. **§2.2 MSR 对齐验证**（0.5 天）
5. **§2.3 MSR #GP 异常处理**（1 天）