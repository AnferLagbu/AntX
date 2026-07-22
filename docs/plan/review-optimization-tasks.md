# 代码审查优化待办

> 2026-07-12 基于全面代码审查产出的优化建议，按优先级整合为可执行待办。
> 2026-07-12 更新: dead code 消除工程已完成大部分可外科手术消除的项。

---

## 一、高优先级

### 1.1 lib.rs clippy allow 收敛

- **描述**: `src/rust/src/lib.rs` 当前有 3 个 clippy allow（均为 `unused_mut`），数量已大幅收敛
- **方案**: 评估剩余 3 个 allow 是否可通过代码重构消除（如 `let mut` 改为 `let`）
- **状态**: [X] 已从 42 个收敛到 3 个

### 1.2 全局可变静态迁移

- **描述**: `static_mut_refs` allow 的根源是 framework 中 38 个 `static mut` 全局可变静态变量，这在 Rust 未来版本中将被废弃
- **方案**: 逐个审计 38 个 `static mut`，按使用场景选择替代方案：单次初始化用 `OnceLock`，并发读写用 `RwLock<T>` 或 `IrqSpinLock<T>`，无竞争场景用 `racy_cell::RacyCell`
- **状态**: [部分完成] 已迁移 CPU_INFO (OnceLock) + KB_READ_SLOT (AtomicU8)，剩余 36 个待迁移

### 已完成的迁移

| 变量 | 原类型 | 新类型 | 文件 |
|------|--------|--------|------|
| `CPU_INFO` | `static mut Option<CpuInfo>` + `AtomicBool` | `OnceLock<CpuInfo>` | `cpu/mod.rs` |
| `KB_READ_SLOT` | `static mut u8` | `AtomicU8` | `driver/input/keyboard.rs` |

### 待迁移 (按复杂度分组)

| 复杂度 | 变量 | 建议替代方案 |
|--------|------|-------------|
| 低 (启动后只读) | VGA_DRIVER, GLOBAL_FRAMEBUFFER, SERIAL_PORTS | `OnceLock` (需 InteriorMutability 包装) |
| 中 (并发读写) | ISR_TABLE, LOG_SINKS, SLAB_CACHES | `IrqSpinLock` 或 `OnceLock` |
| 高 (复杂类型) | NET_DEVICE, NET_STACK, SOCKET_SET, SOCKET_TABLE | 需重构为类型安全全局 |

### 1.3 services/fs 层 KernelError 统一 — ✅ 已完成 (2026-07-21)

- **描述**: `services/fs/vfs_types.rs` 定义了独立的 `KernelError`（15 个变体），与顶层 `services/error.rs` 的 `KernelError`（27 个变体）不一致，增加认知负担
- **方案**: 将 fs 层 `KernelError` 统一到顶层 `services::error::KernelError`，添加 `NotInitialized`/`Overflow` 变体，更新 fs 层所有 `use` 路径
- **状态**: [X] 已完成

---

## 二、中优先级

### 2.1 子系统级性能基准测试

- **描述**: 当前仅有 `framekernel-bench` 全局基线，缺少子系统级细粒度性能数据，难以定位回归来源
- **方案**: 在 `host-tests/benches/` 下增加子系统 benchmark：调度延迟 (context_switch_latency)、IPC 吞吐 (pipe_throughput/shm_throughput)、文件系统 ops/sec (vfs_open_close/read_write)、网络延迟 (loopback_rtt)。输出格式与现有 `baseline.json` 对齐
- **状态**: []

### 2.2 aarch64 QEMU 测试纳入 CI 默认路径

- **描述**: x86_64 有完整的 QEMU 测试链 (test-unit/test-chaos/test-smp)，aarch64 当前仅做编译检查，测试覆盖不对称
- **方案**: 在 `ci/audit.sh` full 模式中增加 aarch64 QEMU boot test；在 `Makefile.ci` 中增加 `ci-test-aarch64` 目标；GitHub Actions `ci-aarch64.yml` 增加 boot test job
- **状态**: []

### 2.3 services 层模块间隐式依赖审计

- **描述**: `audit_coupling.py` 检测模块间循环依赖，但 services 层模块可能通过 framework 全局状态（如 `VFS_MANAGER`、`IPC_NAMESPACE`、`SCHEDULER`）产生隐式依赖，当前无工具检测这类间接耦合
- **方案**: 扩展 `audit_coupling.py` 或新增脚本，扫描 services 层各模块对 framework 全局状态的引用，构建隐式依赖图；输出超过阈值时告警
- **状态**: []

### 2.4 WASM 解释器进度明确化

- **描述**: `services/wasm/` 已完成全量迁移 (interpreter.rs 1111 行 + types.rs 445 行 + module.rs 424 行 + runtime.rs 231 行 + leb128.rs 96 行)，`framework/wasm/` 仅为 re-export shim。`services/wasm/mod.rs` 文档已更新为"已完成迁移 (T6-9)"
- **方案**: 已完成，无需操作
- **状态**: [X] 已完成

### 2.5 future-roadmap.md 重复条目清理

- **描述**: `docs/plan/future-roadmap.md` 中 F2 (RISC-V) 和 F3 (TDX) 各出现两次，内容完全重复
- **方案**: 删除重复的 F2 和 F3 段落，保留各一份
- **状态**: [X] 已完成 (2026-07-12)

---

## 三、低优先级

### 3.1 文档引用行号维护机制

- **描述**: `docs/explain/` 中多处引用源文件具体行号（如 `user_proc.rs:810-858`），代码变动后行号过期，导致文档失实
- **方案**: 改用函数名/类型名引用而非行号，格式改为 `[描述](./path) 的 \`function_name()\` 函数`；对现有文档执行一轮行号校验，过期的改为函数名引用
- **状态**: []

### 3.2 services 层 Thin Wrapper 文档注释

- **描述**: services 层大量文件（lifecycle.rs、execve.rs 等）仅 10-30 行，是对 framework 函数的薄封装。这些文件缺少说明"为什么需要这层封装"的文档
- **方案**: 为 < 30 行的 wrapper 文件在模块级 doc comment 中补充一句说明（如"封装 framework TCB 实现，提供 safe 接口并转换错误类型"）
- **状态**: []

### 3.3 audit_comment_language.py 技术术语白名单更新

- **描述**: 随着新子系统加入（如 WASM、io_uring、eBPF），可能引入新的英文技术术语需要加入白名单
- **方案**: 建立白名单维护流程：新术语出现时在 audit 脚本的 `ALLOWED_TERMS` 中登记，同时在 AGENTS.md 或独立文档中记录白名单变更历史
- **状态**: []

---

## 四、Dead Code 消除工程进展

> 2026-07-12 会话完成的 dead code 消除工作汇总。

### 已完成

| 项 | 方式 | 变化 |
|----|------|------|
| services 模块级 `#![allow(dead_code)]` | 降级为 item-level 或移除 | 8 → 2 (-6) |
| framework 硬件常量接入 | ATA DF/DRDY/ERROR, virtio IOERR/UNSUPP, virtio-net F_STATUS, virtio FAILED/NEEDS_RESET | -12 |
| framework 已使用项移除多余 allow | idt 地址校验, net 12 个翻译函数/stub, cstr as_kstr_opt, proc vmm_switch_page_table, lockdep atomics import | -12 |
| idt 旧异常路径清理 | 移除被 create_handler 取代的 8 个旧方法 + 3 个地址校验函数 + safety.rs 重复 + 测试 + import | -11 |
| elf PT_GNU_STACK 接入 | ELF 加载路径增加 GNU_STACK 段检测 + 日志 | -1 |
| barrier RegisteredDomain::name 接入恢复日志 | cascade_recover/hard_reset_domain 增加域名日志 | -1 |
| racy_cell get_mut 移除多余 allow | 已在 ipc/dynamic.rs 中使用 | -1 |
| dcache 统计函数移除多余 allow | hit_rate/icache_hit_rate/count/icache_count/reset_stats | -5 |
| `/proc/meminfo` Slab+VmallocUsed | 新增 slab_get_stats/kmalloc_get_stats safe 包装 + procfs 集成 | +2 功能 |
| `/proc/stat` softirq tick | tick_query::current_tick() 接入 | +1 功能 |
| `/proc/slabinfo` 逐缓存数据 | slab_get_cache_infos() safe 包装 + procfs 逐缓存输出 | +1 功能 |
| `/proc/fs/dcache` | dcache/icache 统计接口 | +1 功能 |
| future-roadmap.md 重复条目 | 删除 F2/F3 重复段落 | 清理 |
| aarch64 stack_bottom 符号别名 + boot canary | start.S 添加符号, entry.rs 写入 canary | +1 修复 |
| PMM bitmap_size 偏移修复 | p.add(2)→p.add(1) 修正 struct layout 读取 | +1 修复 |
| e1000 probe 运行时安全 | aarch64 返回 -1 而非 cfg-gate 排除 | +1 修复 |
| canary 检查恢复 | aarch64 移除 cfg 跳过, 完整运行 canary 检查 | +1 修复 |
| slab copy_nonoverlapping 移除 | 未使用函数删除 | -1 |
| smoltcp features 扩展 | proto-ipv4-fragmentation + socket-tcp-cubic | +2 功能 |
| smoltcp ipv4.rs mut 修复 | 修复 vendored 代码编译错误 | +1 修复 |

**framework 消除: 27 项, services 消除: 5 项, 总计: 32 项 (+8 功能/修复)**

### 剩余项 (framework 197 + services 71 static_mut_refs)

> **2026-07-21 验证更新**: `#[allow(dead_code)]` 已全部消除（仅剩 `pci/msi.rs` 文件级 allow）。
> 当前主要技术债为 framework 中 38 个 `static mut` 全局可变静态变量（`static_mut_refs` 警告）。

| 类别 | 数量 | 消除条件 |
|------|------|----------|
| framework static mut (arch) | 7 | aarch64 页表 + x86_64 GDT/SMP，需架构特定替代方案 |
| framework static mut (mm) | 8 | kmalloc/slab/VMA/CURRENT_MM，需 OnceLock/RwLock 包装 |
| framework static mut (net) | 13 | NET_DEVICE/SOCKET_SET/SOCKET_TABLE 等，需重构为类型安全全局 |
| framework static mut (driver) | 5 | VGA/SERIAL/FRAMEBUFFER/KALLOC 等，需初始化重构 |
| framework static mut (其他) | 5 | irqline/klog/dma/credo/grant，需逐个评估替代方案 |
| smoltcp 内部 | ~10 | 第三方库豁免 |
| pci/msi.rs 文件级 allow | 1 | 待 MSI/MSI-X 子系统激活后移除 |

---

## 五、已确认保留项（不需要行动）

以下项经审查确认无需修改，记录在此避免重复讨论：

| 项 | 原因 |
|----|------|
| e1000 EERD 常量 dead_code | 仅 `e1000-real-hw` feature 下使用，默认构建 dead 合理 |
| 条件编译 stub (QEMU 路径) | feature gate 导致，默认构建 dead 合理 |

---

## 实施时间线建议

```text
Phase 1 (近期):  1.3 KernelError 统一 + 1.2 static mut 迁移 (38 个 framework static mut)
Phase 2 (中期):  2.1 子系统 benchmark
Phase 3 (远期):  2.2 aarch64 测试 + 2.3 隐式依赖审计
持续:            3.1-3.3 文档与工具维护
```
