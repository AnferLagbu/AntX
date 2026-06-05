# 内核可裁剪性提升计划

> 状态: 规划中 | 创建: 2026-05-23

## 1. 现状评估

### 1.1 当前可配置项

| 层级 | 机制 | 数量 | 说明 |
|------|------|------|------|
| 架构 | `#[cfg(target_arch)]` | x86_64: 190, aarch64: 27 | 编译时按架构选择 |
| Feature | `Cargo.toml [features]` | 3 个 | `kernel_test`, `fault_injection`, `e1000-verbose` |
| 隐式 Feature | 代码中引用但未定义 | `alloc`, `pci`, `smp` 等 | 实际效果为"总是启用" |

### 1.2 架构硬耦合清单

以下子系统通过 `#[cfg(target_arch = "x86_64")]` 与架构绑定，无法独立开关：

| 子系统 | 文件 | 门控方式 | 可独立开关? |
|--------|------|---------|-----------|
| `kernel::net` (lwIP 网络栈) | [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs#L63) | `#[cfg(target_arch = "x86_64")]` | 否 |
| `kernel::pci` (PCI 总线) | [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs#L82) | `#[cfg(target_arch = "x86_64")]` | 否 |
| `kernel::syscall` (系统调用) | [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs#L88) | `#[cfg(target_arch = "x86_64")]` | 否 |
| `kernel::smp` (多核) | [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs#L107) | `#[cfg(target_arch = "x86_64")]` | 否 |
| `kernel::fs::hvfs` (HvFS 文件系统) | [fs/mod.rs](file:///home/anfer/Code/AntX/src/kernel/fs/mod.rs#L3) | `#[cfg(target_arch = "x86_64")]` | 否 |
| `kernel::driver::bus` (总线驱动) | bus/mod.rs | `#[cfg(target_arch = "x86_64")]` | 否 |
| 内核 HvFS 挂载 | [lib.rs](file:///home/anfer/Code/AntX/src/rust/src/lib.rs#L318) | `#[cfg(target_arch = "x86_64")]` | 否 |

### 1.3 能力评估

**综合评级: 低**

- 用户可控编译开关: **仅 3 个**
- 最小内核粒度: **架构级** (x86_64 或 aarch64)
- 子系统开关: **0 个** (无独立 feature gate)
- 无法构建"最小功能内核"用于嵌入式/裁减场景

## 2. 目标

### 2.1 近期目标 (v1)

- 将架构硬耦合的子系统改为 feature gate
- 用户可通过 `cargo build --features=...` 控制子系统启用

### 2.2 中期目标 (v2)

- 原子系统增加独立 feature
- 补齐缺失的 feature 定义 (`alloc`, `pci`, `smp`)
- `kernel_test` gate 细化为 `test_mode` + `test_framework`

### 2.3 远期目标 (v3)

- 实现最小内核配置: `mm + proc + boot` (~100KB)
- Cargo workspace 拆分: 网络栈等独立 crate

## 3. 实施路线

### Phase 1: 架构解耦 → Feature Gate (高优先级)

将 `target_arch = "x86_64"` 改为 feature gate，保持默认行为不变。

```
Cargo.toml:
[features]
default = ["net", "pci", "syscall", "smp", "hvfs"]
net = []       # lwIP 网络协议栈
pci = []       # PCI 总线驱动
syscall = []   # 系统调用接口
smp = []       # 多核支持
hvfs = []      # HvFS 文件系统
```

| 任务 | 影响文件 | 工作量 |
|------|---------|--------|
| [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs): `net` 模块门控改为 `feature = "net"` | 1 文件 | 小 |
| [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs): `pci` 模块门控改为 `feature = "pci"` | 1 文件 | 小 |
| [mod.rs](file:///home/anfer/Code/AntX/src/kernel/mod.rs): `syscall` 模块门控改为 `feature = "syscall"` | 1 文件 | 小 |
| [lib.rs](file:///home/anfer/Code/AntX/src/rust/src/lib.rs): 网络初始化增加 `feature = "net"` gate | 1 文件 | 小 |
| [lib.rs](file:///home/anfer/Code/AntX/src/rust/src/lib.rs): HvFS 挂载增加 `feature = "hvfs"` gate | 1 文件 | 小 |
| 编译验证: `default` + `--no-default-features` 两种配置 | - | 中 |

### Phase 2: 子系统独立 Feature (中优先级)

为更多子系统增加 feature gate，允许按需裁剪：

```
[features]
ipc = []       # 进程间通信
fs = []        # VFS/ramfs/devfs/procfs
driver = []    # 设备驱动子系统
barrier = []   # 故障恢复屏障
dma = []       # DMA 引擎
pwid = []      # 安全框架
```

| 任务 | 说明 | 工作量 |
|------|------|--------|
| `ipc` 模块 feature gate | [ipc/mod.rs](file:///home/anfer/Code/AntX/src/kernel/ipc/mod.rs) | 小 |
| `driver` 模块 feature gate | [driver/mod.rs](file:///home/anfer/Code/AntX/src/kernel/driver/mod.rs) | 中 |
| `fs` 模块 feature gate | [fs/mod.rs](file:///home/anfer/Code/AntX/src/kernel/fs/mod.rs) | 中 |
| `barrier` 模块 feature gate | [barrier/mod.rs](file:///home/anfer/Code/AntX/src/kernel/barrier/mod.rs) | 小 |
| `dma`/`pwid`/`tests` 模块 feature gate | 对应 mod 文件 | 小 |
| 补齐隐式 feature: `alloc`/`pci`/`smp` 定义 | Cargo.toml | 小 |
| 编译验证: 多种 feature 组合 | - | 中 |

### Phase 3: 精细化 (低优先级)

| 任务 | 说明 | 工作量 |
|------|------|--------|
| `kernel_test` 细化为 `test_mode` + `test_framework` | 测试启动与测试框架分离 | 中 |
| 网络栈 `feature` 细化: `net-tcp`/`net-udp`/`net-http` | lwIP 内部 cfg 已支持，暴露为 Rust feature | 大 |
| Cargo workspace 拆分 | 网络/crypto 独立 crate | 大 |
| 嵌入式 Profile 定义 | `cargo build --profile=minimal` | 中 |

## 4. 编译验证矩阵

每种 feature 组合需通过编译 + QEMU 启动测试：

| 配置 | Features | 预期 |
|------|----------|------|
| full (默认) | `default` | 全功能启动 |
| minimal | `--no-default-features` | 仅 mm+proc+boot 启动 |
| net-only | `net` | 无文件系统的网络栈 |
| embedded | `--no-default-features` + 自定义 | 最小内核 < 500KB |

## 5. 风险与约束

| 风险 | 缓解 |
|------|------|
| feature 组合爆炸 (n 个 feature = 2^n 组合) | 只测试 default + minimal 两条路径 |
| 跨模块依赖导致编译失败 | 为需要上游模块的 feature 添加隐式依赖 (如 `fs` 隐式依赖 `alloc`) |
| lwIP C 库无法跟随 feature 裁剪 | Phase 1 只控制 Rust 侧的模块声明，C 侧暂不变 |
| aarch64 缺网络/PCI → feature gate 对 aarch64 无意义 | 维持 aarch64 的 `#[cfg(not(target_arch = "aarch64"))]` 保护 |

## 6. 相关文档

- [架构移植指南](file:///home/anfer/Code/AntX/docs/explain/arch-porting.md)
- [多架构解耦计划](file:///home/anfer/Code/AntX/docs/plan/multiarch-decoupling.md)
- [构建系统](file:///home/anfer/Code/AntX/docs/explain/build-system.md)